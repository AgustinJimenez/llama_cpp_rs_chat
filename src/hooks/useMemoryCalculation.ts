import { useMemo } from 'react';
import type { MemoryBreakdown } from '@/components/organisms/model-config/MemoryVisualization';
import type { ModelMetadata } from '@/types';
import { getKvCacheLayers, calculateKvCacheGb } from '@/utils/vramUtils';

/** VRAM overhead for CUDA context, compute buffers, scratch/activation memory.
 *  Matches the optimizer's VRAM_HEADROOM_GB so the visual bar and auto-fit agree. */
const DEFAULT_OVERHEAD_GB = 2.0;

interface MemoryCalculationParams {
  modelMetadata: ModelMetadata | null;
  gpuLayers: number;
  contextSize: number;
  availableVramGb: number;
  availableRamGb: number;
  overheadGb?: number;
  cacheTypeK?: string;
  cacheTypeV?: string;
}

interface ArchitectureParams {
  totalLayers: number;
  kvAttentionLayers: number;
  modelSizeGb: number;
  qHeads: number;
  kvHeads: number;
  embeddingLength: number;
  headDimK?: number;  // Explicit key head dim from GGUF (overrides embeddingLength/qHeads)
  headDimV?: number;  // Explicit value head dim from GGUF
  slidingWindow?: number;
  perLayerKvHeads?: number[];
}

/** Parse a metadata string field into a number, or return null */
function parseField(value: string | undefined): number | null {
  if (!value) return null;
  const n = parseInt(value);
  return isNaN(n) ? null : n;
}

/** Extract architecture parameters from model metadata with fallbacks */
export function extractArchitectureParams(meta: ModelMetadata): ArchitectureParams {
  const totalLayers =
    meta.architecture_details?.block_count ||
    parseField(meta.block_count) ||
    meta.estimated_layers ||
    48;

  const qHeads =
    meta.architecture_details?.attention_head_count ||
    parseField(meta.attention_head_count) ||
    32;

  const kvHeads =
    meta.architecture_details?.attention_head_count_kv ||
    parseField(meta.attention_head_count_kv) ||
    qHeads; // Default: same as Q heads (no GQA)

  const embeddingLength =
    meta.architecture_details?.embedding_length ||
    parseField(meta.embedding_length) ||
    4096;

  const kvAttentionLayers = getKvCacheLayers(meta);

  // Check for explicit key/value head dimensions in GGUF metadata.
  // Some architectures (e.g. Qwen3.5-35B-A3B) have key_length=256 but
  // embeddingLength/qHeads=128, making the derived headDim wrong by 2x.
  const gguf = meta.gguf_metadata || {};
  let headDimK: number | undefined;
  let headDimV: number | undefined;
  for (const key of Object.keys(gguf)) {
    if (key.endsWith('.attention.key_length')) {
      const v = Number(gguf[key]);
      if (v > 0 && isFinite(v)) headDimK = v;
    }
    if (key.endsWith('.attention.value_length')) {
      const v = Number(gguf[key]);
      if (v > 0 && isFinite(v)) headDimV = v;
    }
  }

  // SWA: extract sliding_window and per-layer KV head counts
  let slidingWindow: number | undefined;
  let perLayerKvHeads: number[] | undefined;
  for (const key of Object.keys(gguf)) {
    if (key.endsWith('.attention.sliding_window') || key === 'attention.sliding_window') {
      const v = Number(gguf[key]);
      if (v > 0 && isFinite(v)) slidingWindow = v;
    }
    if (key.endsWith('.attention.head_count_kv') || key === 'attention.head_count_kv') {
      const val = gguf[key];
      if (Array.isArray(val)) {
        perLayerKvHeads = val.map(Number).filter(n => isFinite(n));
      }
    }
  }

  return {
    totalLayers,
    kvAttentionLayers,
    modelSizeGb: meta.file_size_gb || 0,
    qHeads,
    kvHeads,
    embeddingLength,
    headDimK,
    headDimV,
    slidingWindow,
    perLayerKvHeads,
  };
}

const EMPTY_BREAKDOWN = (vramGb: number, ramGb: number): MemoryBreakdown => ({
  vram: {
    total: vramGb,
    modelGpu: 0,
    kvCache: 0,
    overhead: DEFAULT_OVERHEAD_GB,
    available: vramGb - DEFAULT_OVERHEAD_GB,
    overcommitted: false,
  },
  ram: {
    total: ramGb,
    modelCpu: 0,
    available: ramGb,
    overcommitted: false,
  },
});

/**
 * Real-time memory calculation hook
 * Recalculates memory breakdown whenever inputs change
 */
export function useMemoryCalculation({
  modelMetadata,
  gpuLayers,
  contextSize,
  availableVramGb,
  availableRamGb,
  overheadGb = DEFAULT_OVERHEAD_GB,
  cacheTypeK = 'f16',
  cacheTypeV = 'f16',
}: MemoryCalculationParams): MemoryBreakdown {
  return useMemo(() => {
    if (!modelMetadata) {
      return EMPTY_BREAKDOWN(availableVramGb, availableRamGb);
    }

    const { totalLayers, kvAttentionLayers, modelSizeGb, qHeads, kvHeads, embeddingLength, headDimK, headDimV, slidingWindow, perLayerKvHeads } =
      extractArchitectureParams(modelMetadata);

    // Calculate how much of model goes to GPU vs CPU
    const gpuLayersClamped = Math.min(Math.max(gpuLayers, 0), totalLayers);
    const gpuFraction = gpuLayersClamped / totalLayers;

    const modelGpuSizeGb = modelSizeGb * gpuFraction;
    const modelCpuSizeGb = modelSizeGb * (1 - gpuFraction);

    const kvCacheSizeGb = calculateKvCacheGb(
      contextSize, kvAttentionLayers, kvHeads, qHeads, embeddingLength,
      cacheTypeK, cacheTypeV, headDimK, headDimV,
      slidingWindow, perLayerKvHeads,
    );

    const vramUsed = modelGpuSizeGb + kvCacheSizeGb + overheadGb;
    const ramUsed = modelCpuSizeGb;

    return {
      vram: {
        total: availableVramGb,
        modelGpu: modelGpuSizeGb,
        kvCache: kvCacheSizeGb,
        overhead: overheadGb,
        available: Math.max(0, availableVramGb - vramUsed),
        overcommitted: vramUsed > availableVramGb,
      },
      ram: {
        total: availableRamGb,
        modelCpu: modelCpuSizeGb,
        available: Math.max(0, availableRamGb - ramUsed),
        overcommitted: ramUsed > availableRamGb,
      },
    };
  }, [modelMetadata, gpuLayers, contextSize, availableVramGb, availableRamGb, overheadGb, cacheTypeK, cacheTypeV]);
}
