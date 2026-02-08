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

/**
 * Calculate KV cache size in GB
 * Formula: context × layers × kv_heads × head_dim × 2 (K+V) × 2 bytes (fp16)
 * where head_dim = embedding_length / q_heads (NOT kv_heads)
 */
function calculateKvCacheSize(
  contextSize: number,
  totalLayers: number,
  kvHeads: number,
  qHeads: number,
  embeddingLength: number
): number {
  // head_dim is determined by total Q attention heads, not KV heads
  const headDim = embeddingLength / qHeads;

  // KV cache: each layer stores K and V projections of size (kv_heads × head_dim) per token
  // 2 = K + V, 2 = bytes per element (fp16)
  const bytes = contextSize * totalLayers * kvHeads * headDim * 2 * 2;

  // Convert to GB
  return bytes / (1024 * 1024 * 1024);
}

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
    // Default values if no model metadata available
    if (!modelMetadata) {
      return {
        vram: {
          total: availableVramGb,
          modelGpu: 0,
          kvCache: 0,
          overhead: 2.0,
          available: availableVramGb - 2.0,
          overcommitted: false,
        },
        ram: {
          total: availableRamGb,
          modelCpu: 0,
          available: availableRamGb,
          overcommitted: false,
        },
      };
    }

    // Extract architecture details with fallbacks
    const totalLayers =
      modelMetadata.architecture_details?.block_count ||
      (modelMetadata.block_count ? parseInt(modelMetadata.block_count) : null) ||
      modelMetadata.estimated_layers ||
      48;

    const modelSizeGb = modelMetadata.file_size_gb || 0;

    const qHeads =
      modelMetadata.architecture_details?.attention_head_count ||
      (modelMetadata.attention_head_count ? parseInt(modelMetadata.attention_head_count) : null) ||
      32;

    const kvHeads =
      modelMetadata.architecture_details?.attention_head_count_kv ||
      (modelMetadata.attention_head_count_kv ? parseInt(modelMetadata.attention_head_count_kv) : null) ||
      qHeads; // Default: same as Q heads (no GQA)

    const embeddingLength =
      modelMetadata.architecture_details?.embedding_length ||
      (modelMetadata.embedding_length ? parseInt(modelMetadata.embedding_length) : null) ||
      4096;

    // Calculate how much of model goes to GPU vs CPU
    const gpuLayersClamped = Math.min(Math.max(gpuLayers, 0), totalLayers);
    const cpuLayers = totalLayers - gpuLayersClamped;

    const gpuFraction = gpuLayersClamped / totalLayers;
    const cpuFraction = cpuLayers / totalLayers;

    const modelGpuSizeGb = modelSizeGb * gpuFraction;
    const modelCpuSizeGb = modelSizeGb * cpuFraction;

    // Calculate KV cache size (only on GPU)
    const kvCacheSizeGb = calculateKvCacheSize(
      contextSize,
      totalLayers,
      kvHeads,
      qHeads,
      embeddingLength
    );

    // System overhead (buffers, context management, etc.)
    const overheadGb = 2.0;

    // VRAM calculations
    const vramUsed = modelGpuSizeGb + kvCacheSizeGb + overheadGb;
    const vramAvailable = Math.max(0, availableVramGb - vramUsed);
    const vramOvercommitted = vramUsed > availableVramGb;

    // RAM calculations
    const ramUsed = modelCpuSizeGb;
    const ramAvailable = Math.max(0, availableRamGb - ramUsed);
    const ramOvercommitted = ramUsed > availableRamGb;

    return {
      vram: {
        total: availableVramGb,
        modelGpu: modelGpuSizeGb,
        kvCache: kvCacheSizeGb,
        overhead: overheadGb,
        available: vramAvailable,
        overcommitted: vramOvercommitted,
      },
      ram: {
        total: availableRamGb,
        modelCpu: modelCpuSizeGb,
        available: ramAvailable,
        overcommitted: ramOvercommitted,
      },
    };
  }, [modelMetadata, gpuLayers, contextSize, availableVramGb, availableRamGb]);
}
