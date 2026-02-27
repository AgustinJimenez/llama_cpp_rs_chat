// Shared VRAM calculation utilities for memory visualization and auto-optimization.
// Used by useMemoryCalculation (live memory bars) and useVramOptimizer (smart defaults).

import type { ModelMetadata } from '@/types';

/**
 * Determine how many layers carry KV cache for a given model architecture.
 * Hybrid models (e.g., Qwen3.5-35B-A3B) interleave attention layers with
 * SSM/DeltaNet layers — only the attention layers allocate KV cache.
 */
export function getKvCacheLayers(meta: ModelMetadata): number {
  const totalLayers =
    meta.architecture_details?.block_count ||
    parseInt(meta.block_count || '0') ||
    meta.estimated_layers ||
    48;

  const arch = (meta.architecture || '').toLowerCase();
  const gguf = meta.gguf_metadata || {};

  // Hybrid architectures: check for full_attention_interval in GGUF metadata.
  // Key format: {arch}.full_attention_interval (e.g., "qwen35moe.full_attention_interval")
  // Value N means every Nth layer is a full-attention layer.
  for (const key of Object.keys(gguf)) {
    if (key.endsWith('.full_attention_interval')) {
      const interval = Number(gguf[key]);
      if (interval > 0 && isFinite(interval)) {
        return Math.ceil(totalLayers / interval);
      }
    }
  }

  // Pure recurrent architectures: no KV cache at all
  if (arch.includes('mamba') || arch.includes('rwkv')) {
    return 0;
  }

  // Standard transformer: all layers have KV cache
  return totalLayers;
}

/** Bytes per element for a given KV cache quantization type. */
export function getCacheBytesPerElement(cacheType: string): number {
  switch (cacheType.toLowerCase()) {
    case 'q4_0':
    case 'q4_1':
      return 0.5625; // 4.5 bits = 0.5625 bytes (4-bit + 0.5-bit scale overhead)
    case 'q5_0':
    case 'q5_1':
      return 0.6875; // 5.5 bits
    case 'q8_0':
      return 1.0625; // 8.5 bits (8-bit + 0.5-bit scale)
    case 'f32':
      return 4.0;
    case 'f16':
    default:
      return 2.0;
  }
}

/**
 * Calculate KV cache size in GB, accounting for:
 * - Architecture-specific attention layer count (not all layers have KV cache)
 * - Cache quantization type (f16, q8_0, q4_0, etc.)
 *
 * Formula per cache (K or V):
 *   contextSize × kvAttentionLayers × kvHeads × headDim × bytesPerElement
 */
export function calculateKvCacheGb(
  contextSize: number,
  kvAttentionLayers: number,
  kvHeads: number,
  qHeads: number,
  embeddingLength: number,
  cacheTypeK: string = 'f16',
  cacheTypeV: string = 'f16',
): number {
  if (kvAttentionLayers <= 0 || kvHeads <= 0 || qHeads <= 0) return 0;
  const headDim = embeddingLength / qHeads;
  const bytesK = contextSize * kvAttentionLayers * kvHeads * headDim
    * getCacheBytesPerElement(cacheTypeK);
  const bytesV = contextSize * kvAttentionLayers * kvHeads * headDim
    * getCacheBytesPerElement(cacheTypeV);
  return (bytesK + bytesV) / (1024 * 1024 * 1024);
}
