import { useMemo } from 'react';
import type { ModelMetadata } from '@/types';
import { calculateKvCacheGb } from '@/utils/vramUtils';
import { extractArchitectureParams } from '@/hooks/useMemoryCalculation';

interface VramOptimizerParams {
  modelMetadata: ModelMetadata | null;
  availableVramGb: number;
  maxLayers: number;
  cacheTypeK: string;
  cacheTypeV: string;
  presetContextSize: number | undefined;
  maxContextSize: number;
}

interface VramOptimizerResult {
  optimalGpuLayers: number;
  optimalContextSize: number;
  kvAttentionLayers: number;
  ready: boolean;
}

// Minimum headroom (GB) for CUDA compute buffers, driver overhead, etc.
// Note: Windows display driver typically reserves 1-2 GB of reported VRAM,
// so the total_vram_gb from the system API is already reduced. This headroom
// covers CUDA scratch memory, activation buffers, and compute overhead.
const VRAM_HEADROOM_GB = 2.0;
// Performance headroom: CUDA needs additional workspace for attention scratch
// buffers. When VRAM is too tight, CUDA falls back to slower memory-efficient
// kernels (e.g. 60 tok/s instead of 127 tok/s). This factor ensures the
// optimizer leaves enough room for full-speed generation.
const VRAM_PERF_HEADROOM_GB = 1.0;
const MIN_CONTEXT = 2048;

/** Calculate total VRAM usage for a given configuration */
function estimateVramGb(
  modelSizeGb: number,
  gpuLayers: number,
  totalLayers: number,
  contextSize: number,
  kvAttentionLayers: number,
  kvHeads: number,
  qHeads: number,
  embeddingLength: number,
  cacheTypeK: string,
  cacheTypeV: string,
  headDimK?: number,
  headDimV?: number,
): number {
  const gpuFraction = totalLayers > 0 ? gpuLayers / totalLayers : 0;
  const modelGpuGb = modelSizeGb * gpuFraction;
  const kvCacheGb = calculateKvCacheGb(
    contextSize, kvAttentionLayers, kvHeads, qHeads, embeddingLength,
    cacheTypeK, cacheTypeV, headDimK, headDimV,
  );
  return modelGpuGb + kvCacheGb + VRAM_HEADROOM_GB + VRAM_PERF_HEADROOM_GB;
}

/**
 * Find the largest context size that fits in VRAM via binary search.
 * Returns a value rounded down to the nearest 256 tokens.
 */
function findMaxContext(
  modelSizeGb: number,
  gpuLayers: number,
  totalLayers: number,
  kvAttentionLayers: number,
  kvHeads: number,
  qHeads: number,
  embeddingLength: number,
  cacheTypeK: string,
  cacheTypeV: string,
  availableVramGb: number,
  maxContext: number,
  headDimK?: number,
  headDimV?: number,
): number {
  let lo = MIN_CONTEXT;
  let hi = maxContext;
  let best = MIN_CONTEXT;

  while (lo <= hi) {
    const mid = Math.floor((lo + hi) / 2);
    const rounded = Math.floor(mid / 256) * 256;
    const vram = estimateVramGb(
      modelSizeGb, gpuLayers, totalLayers, rounded,
      kvAttentionLayers, kvHeads, qHeads, embeddingLength,
      cacheTypeK, cacheTypeV, headDimK, headDimV,
    );
    if (vram <= availableVramGb) {
      best = rounded;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }

  return Math.max(best, MIN_CONTEXT);
}

/**
 * Auto-calculate optimal gpu_layers and context_size for the available VRAM.
 *
 * Strategy:
 * 1. Start with all layers on GPU at the target context size.
 * 2. If it fits, done.
 * 3. If not, reduce gpu_layers until it fits.
 * 4. If gpu_layers reaches 0 but the model alone fits, put all layers back
 *    and reduce context size instead (binary search).
 */
export function useVramOptimizer({
  modelMetadata,
  availableVramGb,
  maxLayers,
  cacheTypeK,
  cacheTypeV,
  presetContextSize,
  maxContextSize,
}: VramOptimizerParams): VramOptimizerResult {
  return useMemo(() => {
    if (!modelMetadata || maxLayers <= 0) {
      return { optimalGpuLayers: 0, optimalContextSize: 32768, kvAttentionLayers: 0, ready: false };
    }
    // No GPU detected — force CPU-only mode. `ready: true` so the auto-applier
    // picks this up and writes gpu_layers=0 into the config; otherwise the
    // model load would send gpu_layers=N to llama.cpp and fail with NULL.
    if (availableVramGb <= 0) {
      return { optimalGpuLayers: 0, optimalContextSize: Math.min(32768, maxContextSize), kvAttentionLayers: 0, ready: true };
    }

    const { totalLayers, kvAttentionLayers, modelSizeGb, qHeads, kvHeads, embeddingLength, headDimK, headDimV } =
      extractArchitectureParams(modelMetadata);

    const targetContext = presetContextSize || Math.min(maxContextSize, 32768);

    // Try all layers on GPU at target context
    let gpuLayers = totalLayers;
    let contextSize = targetContext;

    let vram = estimateVramGb(
      modelSizeGb, gpuLayers, totalLayers, contextSize,
      kvAttentionLayers, kvHeads, qHeads, embeddingLength,
      cacheTypeK, cacheTypeV, headDimK, headDimV,
    );

    if (vram <= availableVramGb) {
      // Fits! Try to increase context beyond preset if there's room
      if (contextSize < maxContextSize) {
        const maxCtx = findMaxContext(
          modelSizeGb, gpuLayers, totalLayers,
          kvAttentionLayers, kvHeads, qHeads, embeddingLength,
          cacheTypeK, cacheTypeV, availableVramGb, maxContextSize,
          headDimK, headDimV,
        );
        // Only use larger context if it's meaningfully bigger (>25% more)
        // Otherwise stick with the preset value which is tested/recommended
        if (maxCtx > contextSize * 1.25) {
          contextSize = maxCtx;
        }
      }
      return { optimalGpuLayers: gpuLayers, optimalContextSize: contextSize, kvAttentionLayers, ready: true };
    }

    // Doesn't fit — reduce gpu_layers
    while (gpuLayers > 0) {
      gpuLayers--;
      vram = estimateVramGb(
        modelSizeGb, gpuLayers, totalLayers, contextSize,
        kvAttentionLayers, kvHeads, qHeads, embeddingLength,
        cacheTypeK, cacheTypeV, headDimK, headDimV,
      );
      if (vram <= availableVramGb) {
        return { optimalGpuLayers: gpuLayers, optimalContextSize: contextSize, kvAttentionLayers, ready: true };
      }
    }

    // gpu_layers hit 0 — check if model itself fits on GPU with reduced context
    const modelOnlyVram = modelSizeGb + VRAM_HEADROOM_GB;
    if (modelOnlyVram <= availableVramGb) {
      gpuLayers = totalLayers;
      contextSize = findMaxContext(
        modelSizeGb, gpuLayers, totalLayers,
        kvAttentionLayers, kvHeads, qHeads, embeddingLength,
        cacheTypeK, cacheTypeV, availableVramGb, targetContext,
        headDimK, headDimV,
      );
      return { optimalGpuLayers: gpuLayers, optimalContextSize: contextSize, kvAttentionLayers, ready: true };
    }

    // Model too large for GPU even alone — split layers and use minimum context
    gpuLayers = totalLayers;
    while (gpuLayers > 0) {
      gpuLayers--;
      vram = estimateVramGb(
        modelSizeGb, gpuLayers, totalLayers, MIN_CONTEXT,
        kvAttentionLayers, kvHeads, qHeads, embeddingLength,
        cacheTypeK, cacheTypeV, headDimK, headDimV,
      );
      if (vram <= availableVramGb) {
        // Found gpu_layers that fits at min context, now find max context for this layer count
        contextSize = findMaxContext(
          modelSizeGb, gpuLayers, totalLayers,
          kvAttentionLayers, kvHeads, qHeads, embeddingLength,
          cacheTypeK, cacheTypeV, availableVramGb, targetContext,
          headDimK, headDimV,
        );
        return { optimalGpuLayers: gpuLayers, optimalContextSize: contextSize, kvAttentionLayers, ready: true };
      }
    }

    // Nothing fits — CPU only with minimum context
    return { optimalGpuLayers: 0, optimalContextSize: MIN_CONTEXT, kvAttentionLayers, ready: true };
  }, [modelMetadata, availableVramGb, maxLayers, cacheTypeK, cacheTypeV, presetContextSize, maxContextSize]);
}
