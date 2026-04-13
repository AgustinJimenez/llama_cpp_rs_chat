import { useMemo } from 'react';

const CONTEXT_ROUND_STEP = 256;
const DEFAULT_TARGET_CONTEXT = 32768;
const CONTEXT_GROWTH_THRESHOLD = 1.25;

import { extractArchitectureParams } from '@/hooks/useMemoryCalculation';
import type { ModelMetadata } from '@/types';
import { calculateKvCacheGb } from '@/utils/vramUtils';

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
    contextSize,
    kvAttentionLayers,
    kvHeads,
    qHeads,
    embeddingLength,
    cacheTypeK,
    cacheTypeV,
    headDimK,
    headDimV,
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
    const rounded = Math.floor(mid / CONTEXT_ROUND_STEP) * CONTEXT_ROUND_STEP;
    const vram = estimateVramGb(
      modelSizeGb,
      gpuLayers,
      totalLayers,
      rounded,
      kvAttentionLayers,
      kvHeads,
      qHeads,
      embeddingLength,
      cacheTypeK,
      cacheTypeV,
      headDimK,
      headDimV,
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

interface ArchParams {
  totalLayers: number;
  kvAttentionLayers: number;
  modelSizeGb: number;
  qHeads: number;
  kvHeads: number;
  embeddingLength: number;
  headDimK?: number;
  headDimV?: number;
}

/** Helper that wraps estimateVramGb with fixed architecture params */
function makeVramEstimator(arch: ArchParams, cacheTypeK: string, cacheTypeV: string) {
  return (gpuLayers: number, contextSize: number) =>
    estimateVramGb(
      arch.modelSizeGb,
      gpuLayers,
      arch.totalLayers,
      contextSize,
      arch.kvAttentionLayers,
      arch.kvHeads,
      arch.qHeads,
      arch.embeddingLength,
      cacheTypeK,
      cacheTypeV,
      arch.headDimK,
      arch.headDimV,
    );
}

/** Helper that wraps findMaxContext with fixed architecture params */
function makeContextFinder(arch: ArchParams, cacheTypeK: string, cacheTypeV: string) {
  return (gpuLayers: number, vramGb: number, maxCtx: number) =>
    findMaxContext(
      arch.modelSizeGb,
      gpuLayers,
      arch.totalLayers,
      arch.kvAttentionLayers,
      arch.kvHeads,
      arch.qHeads,
      arch.embeddingLength,
      cacheTypeK,
      cacheTypeV,
      vramGb,
      maxCtx,
      arch.headDimK,
      arch.headDimV,
    );
}

/** Try fitting all layers with target context, optionally expanding context if room */
function tryFullGpu(
  arch: ArchParams,
  targetContext: number,
  maxContextSize: number,
  availableVramGb: number,
  estimate: ReturnType<typeof makeVramEstimator>,
  findCtx: ReturnType<typeof makeContextFinder>,
): VramOptimizerResult | null {
  const vram = estimate(arch.totalLayers, targetContext);
  if (vram > availableVramGb) return null;

  let contextSize = targetContext;
  if (contextSize < maxContextSize) {
    const maxCtx = findCtx(arch.totalLayers, availableVramGb, maxContextSize);
    if (maxCtx > contextSize * CONTEXT_GROWTH_THRESHOLD) {
      contextSize = maxCtx;
    }
  }
  return {
    optimalGpuLayers: arch.totalLayers,
    optimalContextSize: contextSize,
    kvAttentionLayers: arch.kvAttentionLayers,
    ready: true,
  };
}

/** Reduce gpu_layers until the given context fits */
function reduceLayersToFit(
  arch: ArchParams,
  contextSize: number,
  availableVramGb: number,
  estimate: ReturnType<typeof makeVramEstimator>,
): number | null {
  for (let layers = arch.totalLayers - 1; layers >= 0; layers--) {
    if (estimate(layers, contextSize) <= availableVramGb) return layers;
  }
  return null;
}

/** Core VRAM optimization: pure function, no React. */
function calculateOptimalConfig(
  modelMetadata: ModelMetadata,
  availableVramGb: number,
  cacheTypeK: string,
  cacheTypeV: string,
  presetContextSize: number | undefined,
  maxContextSize: number,
): VramOptimizerResult {
  const arch = extractArchitectureParams(modelMetadata);
  const estimate = makeVramEstimator(arch, cacheTypeK, cacheTypeV);
  const findCtx = makeContextFinder(arch, cacheTypeK, cacheTypeV);
  const targetContext = presetContextSize || Math.min(maxContextSize, DEFAULT_TARGET_CONTEXT);

  // Step 1: Try all layers on GPU at target context
  const fullGpu = tryFullGpu(
    arch,
    targetContext,
    maxContextSize,
    availableVramGb,
    estimate,
    findCtx,
  );
  if (fullGpu) return fullGpu;

  // Step 2: Reduce gpu_layers to fit target context
  const reducedLayers = reduceLayersToFit(arch, targetContext, availableVramGb, estimate);
  if (reducedLayers !== null) {
    return {
      optimalGpuLayers: reducedLayers,
      optimalContextSize: targetContext,
      kvAttentionLayers: arch.kvAttentionLayers,
      ready: true,
    };
  }

  // Step 3: Model alone fits — all layers, reduced context
  const modelOnlyVram = arch.modelSizeGb + VRAM_HEADROOM_GB;
  if (modelOnlyVram <= availableVramGb) {
    const contextSize = findCtx(arch.totalLayers, availableVramGb, targetContext);
    return {
      optimalGpuLayers: arch.totalLayers,
      optimalContextSize: contextSize,
      kvAttentionLayers: arch.kvAttentionLayers,
      ready: true,
    };
  }

  // Step 4: Model too large — split layers with minimum context, then maximize context
  const splitLayers = reduceLayersToFit(arch, MIN_CONTEXT, availableVramGb, estimate);
  if (splitLayers !== null) {
    const contextSize = findCtx(splitLayers, availableVramGb, targetContext);
    return {
      optimalGpuLayers: splitLayers,
      optimalContextSize: contextSize,
      kvAttentionLayers: arch.kvAttentionLayers,
      ready: true,
    };
  }

  // Nothing fits — CPU only
  return {
    optimalGpuLayers: 0,
    optimalContextSize: MIN_CONTEXT,
    kvAttentionLayers: arch.kvAttentionLayers,
    ready: true,
  };
}

/**
 * Auto-calculate optimal gpu_layers and context_size for the available VRAM.
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
    if (availableVramGb <= 0) {
      return {
        optimalGpuLayers: 0,
        optimalContextSize: Math.min(DEFAULT_TARGET_CONTEXT, maxContextSize),
        kvAttentionLayers: 0,
        ready: true,
      };
    }
    return calculateOptimalConfig(
      modelMetadata,
      availableVramGb,
      cacheTypeK,
      cacheTypeV,
      presetContextSize,
      maxContextSize,
    );
  }, [
    modelMetadata,
    availableVramGb,
    maxLayers,
    cacheTypeK,
    cacheTypeV,
    presetContextSize,
    maxContextSize,
  ]);
}
