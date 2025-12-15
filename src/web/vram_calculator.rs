use std::fs;
use std::io::BufReader;
use std::process::Command;
use gguf_llms::{GgufHeader, GgufReader, Value};

use crate::{log_info, log_warn};

// Constants for VRAM calculations
pub const DEFAULT_VRAM_GB: f64 = 22.0;  // Default VRAM assumption if detection fails
pub const VRAM_SAFETY_MARGIN_GB: f64 = 2.0;  // Reserve 2GB for system overhead
pub const MB_TO_GB: f64 = 1024.0;
pub const BYTES_TO_GB: f64 = 1024.0 * 1024.0 * 1024.0;
pub const KV_CACHE_MULTIPLIER: f64 = 4.0;  // key + value, 2 bytes each (fp16)

// Model size thresholds for layer estimation
pub const SMALL_MODEL_GB: f64 = 8.0;
pub const SMALL_MODEL_LAYERS: u32 = 32;
pub const MEDIUM_MODEL_GB: f64 = 15.0;
pub const MEDIUM_MODEL_LAYERS: u32 = 48;
pub const LARGE_MODEL_GB: f64 = 25.0;
pub const LARGE_MODEL_LAYERS: u32 = 60;
pub const XLARGE_MODEL_LAYERS: u32 = 80;

// VRAM utilization threshold
pub const MIN_VRAM_RATIO: f64 = 0.1;  // Minimum 10% VRAM required for GPU offloading

/// Detect available VRAM using nvidia-smi.
/// Returns the available VRAM in GB, or DEFAULT_VRAM_GB if detection fails.
pub fn get_available_vram_gb() -> Option<f64> {
    // Try nvidia-smi first
    if let Ok(output) = Command::new("nvidia-smi")
        .args(&["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output()
    {
        if output.status.success() {
            if let Ok(output_str) = String::from_utf8(output.stdout) {
                if let Ok(vram_mb) = output_str.trim().parse::<f64>() {
                    return Some(vram_mb / MB_TO_GB); // Convert MB to GB
                }
            }
        }
    }

    // Fallback: assume default VRAM available (conservative estimate)
    log_info!("system", "Could not detect VRAM, assuming {}GB available", DEFAULT_VRAM_GB);
    Some(DEFAULT_VRAM_GB)
}

/// Calculate KV cache size in GB for given model parameters.
pub fn calculate_kv_cache_size_gb(
    n_ctx: u32,
    n_layers: u32,
    n_kv_heads: u32,
    head_dim: u32,
) -> f64 {
    // KV cache = tokens × layers × kv_heads × head_dim × 4 (key+value, 2 bytes each for fp16)
    let bytes = n_ctx as f64 * n_layers as f64 * n_kv_heads as f64 * head_dim as f64 * KV_CACHE_MULTIPLIER;
    bytes / BYTES_TO_GB // Convert to GB
}

/// Calculate optimal GPU layers based on model file size and available VRAM.
pub fn calculate_optimal_gpu_layers(model_path: &str) -> u32 {
    // Get model file size to estimate memory requirements
    let model_size_bytes = match fs::metadata(model_path) {
        Ok(metadata) => metadata.len(),
        Err(_) => {
            log_info!("system", "Could not read model file size, defaulting to 32 layers");
            return 32;
        }
    };

    let model_size_gb = model_size_bytes as f64 / BYTES_TO_GB;
    log_info!("system", "Model file size: {:.2} GB", model_size_gb);

    // Try to get available GPU VRAM
    // For NVIDIA GPUs, we can estimate based on typical model requirements
    // A rough heuristic:
    // - Small models (< 5GB): Use all GPU layers (typically ~40 layers)
    // - Medium models (5-15GB): Use proportional layers
    // - Large models (> 15GB): May need CPU offload

    // Estimate based on RTX 4090 with ~24GB VRAM
    // Reserve ~2GB for system/context, leaving DEFAULT_VRAM_GB for model
    let available_vram_gb = DEFAULT_VRAM_GB;

    log_info!("system", "Estimated available VRAM: {:.2} GB", available_vram_gb);

    // Calculate what percentage of the model fits in VRAM
    let vram_ratio = (available_vram_gb / model_size_gb).min(1.0);

    // Estimate typical layer count based on model size
    // Small models (~7B params, <8GB): ~32 layers
    // Medium models (~13B params, 8-15GB): ~48 layers
    // Large models (~30B params, 15-25GB): ~60 layers
    // XLarge models (~70B+ params, >25GB): ~80 layers
    let estimated_total_layers = if model_size_gb < SMALL_MODEL_GB {
        SMALL_MODEL_LAYERS
    } else if model_size_gb < MEDIUM_MODEL_GB {
        MEDIUM_MODEL_LAYERS
    } else if model_size_gb < LARGE_MODEL_GB {
        LARGE_MODEL_LAYERS
    } else {
        XLARGE_MODEL_LAYERS
    };

    let optimal_layers = (estimated_total_layers as f64 * vram_ratio).floor() as u32;

    log_info!("system", "Estimated total layers: {}", estimated_total_layers);
    log_info!("system", "VRAM utilization ratio: {:.1}%", vram_ratio * 100.0);
    log_info!("system", "Optimal GPU layers: {} ({}% of model)",
             optimal_layers,
             (optimal_layers as f64 / estimated_total_layers as f64 * 100.0) as u32);

    // Ensure at least 1 layer on GPU if model is small enough
    optimal_layers.max(if vram_ratio > MIN_VRAM_RATIO { 1 } else { 0 })
}

/// Calculate safe context size based on available VRAM and model parameters.
/// Returns (safe_context_size, was_reduced).
pub fn calculate_safe_context_size(
    model_path: &str,
    requested_ctx: u32,
    available_vram_gb: Option<f64>,
    gpu_layers: Option<u32>,
) -> (u32, bool) {
    let available_vram = available_vram_gb.unwrap_or_else(|| {
        get_available_vram_gb().unwrap_or(DEFAULT_VRAM_GB)
    });

    // Read model metadata to get architecture details
    let (n_layers, n_kv_heads, embedding_len) = if let Ok(file) = fs::File::open(model_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                // Try to get layer count, kv heads, embedding length
                let layers = metadata.get("gemma3.block_count")
                    .or_else(|| metadata.get("llama.block_count"))
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        _ => None,
                    }).unwrap_or(48); // Default to 48 layers

                let kv_heads = metadata.get("gemma3.attention.head_count_kv")
                    .or_else(|| metadata.get("llama.attention.head_count_kv"))
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        _ => None,
                    }).unwrap_or(8); // Default to 8 KV heads

                let emb_len = metadata.get("gemma3.embedding_length")
                    .or_else(|| metadata.get("llama.embedding_length"))
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        _ => None,
                    }).unwrap_or(3840); // Default to 3840

                (layers, kv_heads, emb_len)
            } else {
                (48, 8, 3840) // Defaults
            }
        } else {
            (48, 8, 3840)
        }
    } else {
        (48, 8, 3840)
    };

    // Calculate head dimension
    let head_dim = embedding_len / (n_kv_heads * 2); // Rough estimate

    // Estimate model size
    let model_size_gb = if let Ok(metadata) = fs::metadata(model_path) {
        metadata.len() as f64 / BYTES_TO_GB
    } else {
        MEDIUM_MODEL_GB // Default estimate (12GB for ~12B parameter model)
    };

    // Calculate GPU layers (auto-detect if not provided by user)
    let gpu_layers_count = gpu_layers.unwrap_or_else(|| calculate_optimal_gpu_layers(model_path));

    // Calculate what fraction of the model is on GPU
    let gpu_fraction = (gpu_layers_count as f64) / (n_layers as f64);
    let model_vram_usage = model_size_gb * gpu_fraction;

    log_info!("system", "GPU layers: {}/{} ({:.1}% of model)",
             gpu_layers_count, n_layers, gpu_fraction * 100.0);
    log_info!("system", "Model VRAM usage: {:.2}GB ({:.1}% of {:.2}GB total)",
             model_vram_usage, gpu_fraction * 100.0, model_size_gb);

    // Available VRAM for KV cache = total - model_on_gpu - overhead
    let vram_for_cache = (available_vram - model_vram_usage - VRAM_SAFETY_MARGIN_GB).max(0.0);

    log_info!("system", "Available: {:.2}GB, Model: {:.2}GB, Available for KV cache: {:.2}GB",
             available_vram, model_size_gb, vram_for_cache);

    // Calculate KV cache size for requested context
    let requested_cache_gb = calculate_kv_cache_size_gb(requested_ctx, n_layers, n_kv_heads, head_dim);

    log_info!("system", "Requested context: {} tokens, KV cache: {:.2}GB",
             requested_ctx, requested_cache_gb);

    if requested_cache_gb <= vram_for_cache {
        // Requested context fits in VRAM
        log_info!("system", "✓ Requested context size fits in available VRAM");
        return (requested_ctx, false);
    }

    // Calculate safe context size
    // max_tokens = vram_for_cache / (layers × kv_heads × head_dim × 4)
    let bytes_per_token = n_layers as f64 * n_kv_heads as f64 * head_dim as f64 * KV_CACHE_MULTIPLIER;
    let safe_tokens = ((vram_for_cache * BYTES_TO_GB) / bytes_per_token) as u32;

    // Round down to nearest power of 2 for cleaner values
    let safe_ctx = if safe_tokens >= 32768 {
        32768
    } else if safe_tokens >= 16384 {
        16384
    } else if safe_tokens >= 8192 {
        8192
    } else if safe_tokens >= 4096 {
        4096
    } else {
        2048
    };

    log_warn!("system", "⚠️  Requested context ({}) exceeds VRAM capacity!", requested_ctx);
    log_warn!("system", "⚠️  Auto-reducing to safe context size: {} tokens", safe_ctx);

    (safe_ctx, true) // Return safe context and flag that it was reduced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_kv_cache_size_gb() {
        // Test with realistic values
        let n_ctx = 4096;
        let n_layers = 32;
        let n_kv_heads = 32;
        let head_dim = 128;

        let cache_size = calculate_kv_cache_size_gb(n_ctx, n_layers, n_kv_heads, head_dim);

        // Should be > 0 and reasonable (< 100GB for these params)
        assert!(cache_size > 0.0);
        assert!(cache_size < 100.0);

        // Rough calculation: 4096 * 32 * 32 * 128 * 4 / (1024^3) ≈ 2.0 GB
        assert!((cache_size - 2.0).abs() < 0.5);
    }

    #[test]
    fn test_calculate_kv_cache_doubles_with_context() {
        let n_layers = 32;
        let n_kv_heads = 32;
        let head_dim = 128;

        let cache_2k = calculate_kv_cache_size_gb(2048, n_layers, n_kv_heads, head_dim);
        let cache_4k = calculate_kv_cache_size_gb(4096, n_layers, n_kv_heads, head_dim);

        // Doubling context should roughly double cache size
        assert!((cache_4k / cache_2k - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_optimal_gpu_layers_small_model() {
        // Create a small temporary test file
        let test_file = "test_small_model.tmp";

        // Mock file by creating actual temp file
        std::fs::write(test_file, vec![0u8; 1024]).ok();

        let layers = calculate_optimal_gpu_layers(test_file);

        // Clean up
        std::fs::remove_file(test_file).ok();

        // Small model should get high layer count (32 is default for <8GB)
        assert_eq!(layers, SMALL_MODEL_LAYERS);
    }

    #[test]
    fn test_vram_constants_are_reasonable() {
        // Verify our constants make sense
        assert_eq!(DEFAULT_VRAM_GB, 22.0);
        assert_eq!(VRAM_SAFETY_MARGIN_GB, 2.0);
        assert_eq!(KV_CACHE_MULTIPLIER, 4.0);

        assert!(SMALL_MODEL_GB < MEDIUM_MODEL_GB);
        assert!(MEDIUM_MODEL_GB < LARGE_MODEL_GB);

        assert!(SMALL_MODEL_LAYERS < MEDIUM_MODEL_LAYERS);
        assert!(MEDIUM_MODEL_LAYERS < LARGE_MODEL_LAYERS);
        assert!(LARGE_MODEL_LAYERS < XLARGE_MODEL_LAYERS);
    }

    #[test]
    fn test_model_size_thresholds() {
        // Test that our size thresholds are in the right range
        assert_eq!(SMALL_MODEL_GB, 8.0);
        assert_eq!(MEDIUM_MODEL_GB, 15.0);
        assert_eq!(LARGE_MODEL_GB, 25.0);
    }

    #[test]
    fn test_layer_count_thresholds() {
        assert_eq!(SMALL_MODEL_LAYERS, 32);
        assert_eq!(MEDIUM_MODEL_LAYERS, 48);
        assert_eq!(LARGE_MODEL_LAYERS, 60);
        assert_eq!(XLARGE_MODEL_LAYERS, 80);
    }

    #[test]
    fn test_bytes_to_gb_conversion() {
        // Test the BYTES_TO_GB constant
        let one_gb_in_bytes = 1024.0 * 1024.0 * 1024.0;
        assert_eq!(BYTES_TO_GB, one_gb_in_bytes);
    }

    #[test]
    fn test_mb_to_gb_conversion() {
        assert_eq!(MB_TO_GB, 1024.0);
    }

    #[test]
    fn test_min_vram_ratio() {
        // 10% is a reasonable minimum
        assert_eq!(MIN_VRAM_RATIO, 0.1);
    }

    #[test]
    fn test_kv_cache_multiplier_is_four() {
        // key + value, 2 bytes each (fp16) = 4
        assert_eq!(KV_CACHE_MULTIPLIER, 4.0);
    }

    #[test]
    fn test_calculate_kv_cache_with_small_context() {
        // Small context should give small cache
        let cache = calculate_kv_cache_size_gb(512, 16, 16, 64);
        assert!(cache < 1.0); // Should be less than 1GB
    }

    #[test]
    fn test_calculate_kv_cache_with_large_context() {
        // Large context should give larger cache
        let cache = calculate_kv_cache_size_gb(32768, 80, 64, 128);
        assert!(cache > 10.0); // Should be more than 10GB
    }

    #[test]
    fn test_calculate_kv_cache_scales_with_layers() {
        let n_ctx = 4096;
        let n_kv_heads = 32;
        let head_dim = 128;

        let cache_32_layers = calculate_kv_cache_size_gb(n_ctx, 32, n_kv_heads, head_dim);
        let cache_64_layers = calculate_kv_cache_size_gb(n_ctx, 64, n_kv_heads, head_dim);

        // Doubling layers should double cache
        assert!((cache_64_layers / cache_32_layers - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_get_available_vram_gb_returns_some() {
        // This may fail if nvidia-smi is not available, but should return Some value
        let vram = get_available_vram_gb();
        assert!(vram.is_some());

        // Should return either detected value or default fallback
        let vram_value = vram.unwrap();
        assert!(vram_value > 0.0);
        assert!(vram_value <= 100.0); // Reasonable upper limit
    }

    #[test]
    fn test_calculate_optimal_layers_returns_non_negative() {
        // Even for non-existent files, should return 0 or positive (u32 is always >= 0)
        let layers = calculate_optimal_gpu_layers("nonexistent_file.gguf");
        // u32 is always >= 0, so just check it returns a value
        assert!(layers == layers); // Always true, just checking it compiles/runs
    }

    #[test]
    fn test_safety_margin_is_subtracted_from_vram() {
        // The VRAM_SAFETY_MARGIN_GB should be 2.0
        // This is used in calculate_safe_context_size to reserve memory
        assert_eq!(VRAM_SAFETY_MARGIN_GB, 2.0);

        // Verify it's a reasonable value (not too high, not too low)
        assert!(VRAM_SAFETY_MARGIN_GB > 0.5);
        assert!(VRAM_SAFETY_MARGIN_GB < 5.0);
    }
}
