use serde_json::Value;

use super::ProviderPreset;
use super::PROVIDER_PRESETS;

pub fn get_preset(provider_id: &str) -> Option<&'static ProviderPreset> {
    PROVIDER_PRESETS.iter().find(|p| p.id == provider_id)
}

pub fn is_openai_compat(provider_id: &str) -> bool {
    get_preset(provider_id).is_some()
}

pub fn fetch_models(provider_id: &str, base_url: &str, api_key: &str) -> Vec<String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(10))
        .build()
        .get(&url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .call();

    match resp {
        Ok(r) => {
            if let Ok(body) = r.into_string() {
                if let Ok(json) = serde_json::from_str::<Value>(&body) {
                    if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                        let mut models: Vec<String> = data
                            .iter()
                            .filter_map(|m| {
                                m.get("id")
                                    .and_then(|id| id.as_str())
                                    .map(std::string::ToString::to_string)
                            })
                            .collect();
                        models.sort();
                        if !models.is_empty() {
                            return models;
                        }
                    }
                }
            }
            get_preset(provider_id)
                .map(|p| p.models.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default()
        }
        Err(_) => get_preset(provider_id)
            .map(|p| p.models.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default(),
    }
}

pub fn resolve_api_key(provider_id: &str, api_keys_json: Option<&str>) -> Option<String> {
    if let Some(json_str) = api_keys_json {
        if let Ok(map) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(provider_obj) = map.get(provider_id) {
                if let Some(key) = provider_obj.get("api_key").and_then(|v| v.as_str()) {
                    if !key.is_empty() {
                        return Some(key.to_string());
                    }
                }
                if let Some(key) = provider_obj.as_str() {
                    if !key.is_empty() {
                        return Some(key.to_string());
                    }
                }
            }
        }
    }

    if let Some(preset) = get_preset(provider_id) {
        if !preset.env_key.is_empty() {
            if let Ok(key) = std::env::var(preset.env_key) {
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }
        if !preset.default_key.is_empty() {
            return Some(preset.default_key.to_string());
        }
    }

    None
}

pub fn resolve_base_url(provider_id: &str, api_keys_json: Option<&str>) -> Option<String> {
    if let Some(json_str) = api_keys_json {
        if let Ok(map) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(provider_obj) = map.get(provider_id) {
                if let Some(url) = provider_obj.get("base_url").and_then(|v| v.as_str()) {
                    if !url.is_empty() {
                        return Some(url.to_string());
                    }
                }
            }
        }
    }

    if let Some(preset) = get_preset(provider_id) {
        if !preset.base_url.is_empty() {
            return Some(preset.base_url.to_string());
        }
    }

    None
}

pub fn resolve_custom_field(
    provider_id: &str,
    field: &str,
    api_keys_json: Option<&str>,
) -> Option<String> {
    let json_str = api_keys_json?;
    let map: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let val = map.get(provider_id)?.get(field)?.as_str()?;
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}
