import { useMemo } from 'react';
import type { MemoryBreakdown } from '@/components/organisms/model-config/MemoryVisualization';
import type { ModelMetadata } from '@/types';

interface MemoryCalculationParams {
  modelMetadata: ModelMetadata | null;
  gpuLayers: number;
  contextSize: number;
  availableVramGb: number;
  availableRamGb: number;
}

interface ArchitectureParams {
  totalLayers: number;
  modelSizeGb: number;
  qHeads: number;
  kvHeads: number;
  embeddingLength: number;
}

/** Parse a metadata string field into a number, or return null */
function parseField(value: string | undefined): number | null {
  if (!value) return null;
  const n = parseInt(value);
  return isNaN(n) ? null : n;
}

/** Extract architecture parameters from model metadata with fallbacks */
function extractArchitectureParams(meta: ModelMetadata): ArchitectureParams {
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

  return {
    totalLayers,
    modelSizeGb: meta.file_size_gb || 0,
    qHeads,
    kvHeads,
    embeddingLength,
  };
}

/**
 * Calculate KV cache size in GB
 * Formula: context × layers × kv_heads × head_dim × 2 (K+V) × 2 bytes (fp16)
 * where head_dim = embedding_length / q_heads (NOT kv_heads)
 */
function calculateKvCacheGb(
  contextSize: number,
  totalLayers: number,
  kvHeads: number,
  qHeads: number,
  embeddingLength: number
): number {
  const headDim = embeddingLength / qHeads;
  const bytes = contextSize * totalLayers * kvHeads * headDim * 2 * 2;
  return bytes / (1024 * 1024 * 1024);
}

const EMPTY_BREAKDOWN = (vramGb: number, ramGb: number): MemoryBreakdown => ({
  vram: {
    total: vramGb,
    modelGpu: 0,
    kvCache: 0,
    overhead: 2.0,
    available: vramGb - 2.0,
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
}: MemoryCalculationParams): MemoryBreakdown {
  return useMemo(() => {
    if (!modelMetadata) {
      return EMPTY_BREAKDOWN(availableVramGb, availableRamGb);
    }

    const { totalLayers, modelSizeGb, qHeads, kvHeads, embeddingLength } =
      extractArchitectureParams(modelMetadata);

    // Calculate how much of model goes to GPU vs CPU
    const gpuLayersClamped = Math.min(Math.max(gpuLayers, 0), totalLayers);
    const gpuFraction = gpuLayersClamped / totalLayers;

    const modelGpuSizeGb = modelSizeGb * gpuFraction;
    const modelCpuSizeGb = modelSizeGb * (1 - gpuFraction);

    const kvCacheSizeGb = calculateKvCacheGb(
      contextSize, totalLayers, kvHeads, qHeads, embeddingLength
    );

    const overheadGb = 2.0;
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
  }, [modelMetadata, gpuLayers, contextSize, availableVramGb, availableRamGb]);
}
