import { useMemo } from 'react';
import type { MemoryBreakdown } from '@/components/organisms/model-config/MemoryVisualization';
import type { ModelMetadata } from '@/types';
import { getKvCacheLayers, calculateKvCacheGb } from '@/utils/vramUtils';

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

  return {
    totalLayers,
    kvAttentionLayers,
    modelSizeGb: meta.file_size_gb || 0,
    qHeads,
    kvHeads,
    embeddingLength,
  };
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
  overheadGb: overheadParam = 2.0,
  cacheTypeK = 'f16',
  cacheTypeV = 'f16',
}: MemoryCalculationParams): MemoryBreakdown {
  return useMemo(() => {
    if (!modelMetadata) {
      return EMPTY_BREAKDOWN(availableVramGb, availableRamGb);
    }

    const { totalLayers, kvAttentionLayers, modelSizeGb, qHeads, kvHeads, embeddingLength } =
      extractArchitectureParams(modelMetadata);

    // Calculate how much of model goes to GPU vs CPU
    const gpuLayersClamped = Math.min(Math.max(gpuLayers, 0), totalLayers);
    const gpuFraction = gpuLayersClamped / totalLayers;

    const modelGpuSizeGb = modelSizeGb * gpuFraction;
    const modelCpuSizeGb = modelSizeGb * (1 - gpuFraction);

    const kvCacheSizeGb = calculateKvCacheGb(
      contextSize, kvAttentionLayers, kvHeads, qHeads, embeddingLength,
      cacheTypeK, cacheTypeV,
    );

    const vramUsed = modelGpuSizeGb + kvCacheSizeGb + overheadParam;
    const ramUsed = modelCpuSizeGb;

    return {
      vram: {
        total: availableVramGb,
        modelGpu: modelGpuSizeGb,
        kvCache: kvCacheSizeGb,
        overhead: overheadParam,
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
  }, [modelMetadata, gpuLayers, contextSize, availableVramGb, availableRamGb, overheadParam, cacheTypeK, cacheTypeV]);
}
