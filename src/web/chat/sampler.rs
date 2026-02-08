use llama_cpp_2::{model::LlamaModel, sampling::LlamaSampler};

use super::super::models::SamplerConfig;
use crate::log_info;

/// Common sequence breakers for the DRY anti-repetition sampler.
const DRY_SEQ_BREAKERS: &[&[u8]] = &[b"\n", b".", b",", b"!", b"?", b";", b":", b" "];

/// Create a sampler based on the configuration.
///
/// `model` is needed only for the DRY sampler; pass `None` if unavailable.
pub(crate) fn create_sampler(
    config: &SamplerConfig,
    conversation_id: &str,
    model: Option<&LlamaModel>,
) -> LlamaSampler {
    let use_penalties = config.repeat_penalty > 1.0
        || config.frequency_penalty > 0.0
        || config.presence_penalty > 0.0;

    if use_penalties {
        log_info!(
            conversation_id,
            "Penalties enabled: repeat={}, freq={}, presence={}, last_n={}",
            config.repeat_penalty,
            config.frequency_penalty,
            config.presence_penalty,
            config.penalty_last_n
        );
    }

    /// Push the standard penalty sampler onto a chain when any penalty is active.
    fn push_penalties(samplers: &mut Vec<LlamaSampler>, config: &SamplerConfig) {
        samplers.push(LlamaSampler::penalties(
            config.penalty_last_n,
            config.repeat_penalty as f32,
            config.frequency_penalty as f32,
            config.presence_penalty as f32,
        ));
    }

    /// Push DRY anti-repetition sampler when multiplier > 0 and model is available.
    fn push_dry(
        samplers: &mut Vec<LlamaSampler>,
        config: &SamplerConfig,
        model: Option<&LlamaModel>,
    ) {
        if config.dry_multiplier > 0.0 {
            if let Some(m) = model {
                samplers.push(LlamaSampler::dry(
                    m,
                    config.dry_multiplier as f32,
                    config.dry_base as f32,
                    config.dry_allowed_length,
                    config.dry_penalty_last_n,
                    DRY_SEQ_BREAKERS.iter().copied(),
                ));
            }
        }
    }

    /// Push top-N sigma filter when enabled (value > 0).
    fn push_top_n_sigma(samplers: &mut Vec<LlamaSampler>, config: &SamplerConfig) {
        if config.top_n_sigma > 0.0 {
            samplers.push(LlamaSampler::top_n_sigma(config.top_n_sigma as f32));
        }
    }

    match config.sampler_type.as_str() {
        "Temperature" => {
            log_info!(
                conversation_id,
                "Using Temperature sampler: temp={}, top_p={}, top_k={}",
                config.temperature, config.top_p, config.top_k
            );
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::temp(config.temperature as f32));
            s.push(LlamaSampler::top_k(config.top_k as i32));
            s.push(LlamaSampler::top_p(config.top_p as f32, 1));
            if config.min_p > 0.0 {
                s.push(LlamaSampler::min_p(config.min_p as f32, 1));
            }
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "Mirostat" => {
            log_info!(
                conversation_id,
                "Using Mirostat sampler: tau={}, eta={}",
                config.mirostat_tau, config.mirostat_eta
            );
            // Mirostat is a standalone sampler (doesn't chain well with penalties)
            LlamaSampler::mirostat(
                0,    // n_vocab (0 = auto)
                1234, // seed
                config.mirostat_tau as f32,
                config.mirostat_eta as f32,
                100,  // m
            )
        }

        "TopP" => {
            log_info!(conversation_id, "Using TopP sampler: top_p={}", config.top_p);
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::top_p(config.top_p as f32, 1));
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "TopK" => {
            log_info!(conversation_id, "Using TopK sampler: top_k={}", config.top_k);
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::top_k(config.top_k as i32));
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "Typical" => {
            log_info!(conversation_id, "Using Typical sampler: p={}", config.typical_p);
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::typical(config.typical_p as f32, 1));
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "MinP" => {
            log_info!(conversation_id, "Using MinP sampler: min_p={}", config.min_p);
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::min_p(config.min_p as f32, 1));
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "TempExt" => {
            log_info!(
                conversation_id,
                "Using TempExt (dynamic temperature) sampler: temp={}",
                config.temperature
            );
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            // temp_ext(t, delta, exponent) — delta/exponent not yet exposed in UI
            s.push(LlamaSampler::temp_ext(config.temperature as f32, 0.0, 1.0));
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "ChainTempTopP" => {
            log_info!(
                conversation_id,
                "Using ChainTempTopP: temp={}, top_p={}",
                config.temperature, config.top_p
            );
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::temp(config.temperature as f32));
            s.push(LlamaSampler::top_p(config.top_p as f32, 1));
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "ChainTempTopK" => {
            log_info!(
                conversation_id,
                "Using ChainTempTopK: temp={}, top_k={}",
                config.temperature, config.top_k
            );
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::temp(config.temperature as f32));
            s.push(LlamaSampler::top_k(config.top_k as i32));
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        "ChainFull" => {
            log_info!(
                conversation_id,
                "Using ChainFull: temp={}, top_k={}, top_p={}, min_p={}, typical_p={}",
                config.temperature, config.top_k, config.top_p, config.min_p, config.typical_p
            );
            let mut s: Vec<LlamaSampler> = Vec::new();
            if use_penalties { push_penalties(&mut s, config); }
            push_dry(&mut s, config, model);
            push_top_n_sigma(&mut s, config);
            s.push(LlamaSampler::temp(config.temperature as f32));
            s.push(LlamaSampler::top_k(config.top_k as i32));
            s.push(LlamaSampler::top_p(config.top_p as f32, 1));
            if config.min_p > 0.0 {
                s.push(LlamaSampler::min_p(config.min_p as f32, 1));
            }
            if config.typical_p < 1.0 {
                s.push(LlamaSampler::typical(config.typical_p as f32, 1));
            }
            s.push(LlamaSampler::dist(1234));
            LlamaSampler::chain_simple(s)
        }

        // "Greedy" and any unknown type
        _ => {
            log_info!(conversation_id, "Using Greedy sampler");
            if use_penalties {
                let mut s: Vec<LlamaSampler> = Vec::new();
                push_penalties(&mut s, config);
                push_dry(&mut s, config, model);
                s.push(LlamaSampler::greedy());
                LlamaSampler::chain_simple(s)
            } else {
                LlamaSampler::greedy()
            }
        }
    }
}
