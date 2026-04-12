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

/** Parse "17.4 GB" or "512 MB" string to GB number */
function parseFileSizeString(value: string | undefined): number | null {
  if (!value) return null;
  const match = value.match(/([\d.]+)\s*(GB|MB|TB)/i);
  if (!match) return null;
  const num = parseFloat(match[1]);
  const unit = match[2].toUpperCase();
  if (unit === 'TB') return num * 1024;
  if (unit === 'MB') return num / 1024;
  return num;
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
    modelSizeGb: meta.file_size_gb || parseFileSizeString(meta.file_size) || 0,
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
    overhead: vramGb > 0 ? DEFAULT_OVERHEAD_GB : 0,
    available: Math.max(0, vramGb - (vramGb > 0 ? DEFAULT_OVERHEAD_GB : 0)),
    overcommitted: false,
  },
  ram: {
    total: ramGb,
    modelCpu: 0,
    kvCacheCpu: 0,
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
    const gpuFraction = totalLayers > 0 ? gpuLayersClamped / totalLayers : 0;

    const modelGpuSizeGb = modelSizeGb * gpuFraction;
    const modelCpuSizeGb = modelSizeGb * (1 - gpuFraction);

    const kvCacheTotalGb = calculateKvCacheGb(
      contextSize, kvAttentionLayers, kvHeads, qHeads, embeddingLength,
      cacheTypeK, cacheTypeV, headDimK, headDimV,
      slidingWindow, perLayerKvHeads,
    );
    // Split KV cache the same way as the model: layers on GPU keep their
    // KV state on GPU, layers on CPU keep theirs on CPU. Without this split,
    // a CPU-only configuration would still report KV cache as VRAM usage,
    // which falsely flagged the load as "OVERCOMMITTED" on machines with no
    // GPU at all.
    const kvCacheGpuGb = kvCacheTotalGb * gpuFraction;
    const kvCacheCpuGb = kvCacheTotalGb * (1 - gpuFraction);

    // Overhead (CUDA scratch / activation buffers) only applies when at
    // least one layer is offloaded to GPU. Pure-CPU mode has no CUDA context
    // at all, so charging the user 2 GB of VRAM here is misleading.
    const effectiveOverheadGb = gpuFraction > 0 ? overheadGb : 0;

    const vramUsed = modelGpuSizeGb + kvCacheGpuGb + effectiveOverheadGb;
    const ramUsed = modelCpuSizeGb + kvCacheCpuGb;

    return {
      vram: {
        total: availableVramGb,
        modelGpu: modelGpuSizeGb,
        kvCache: kvCacheGpuGb,
        overhead: effectiveOverheadGb,
        available: Math.max(0, availableVramGb - vramUsed),
        // Don't flag overcommitment when there's no GPU at all (total=0):
        // CPU-only is a valid configuration, not an error.
        overcommitted: availableVramGb > 0 && vramUsed > availableVramGb,
      },
      ram: {
        total: availableRamGb,
        modelCpu: modelCpuSizeGb,
        kvCacheCpu: kvCacheCpuGb,
        available: Math.max(0, availableRamGb - ramUsed),
        overcommitted: availableRamGb > 0 && ramUsed > availableRamGb,
      },
    };
  }, [modelMetadata, gpuLayers, contextSize, availableVramGb, availableRamGb, overheadGb, cacheTypeK, cacheTypeV]);
}
