use std::fs;
use super::models::SamplerConfig;

// Helper function to load configuration
pub fn load_config() -> SamplerConfig {
    let config_path = "assets/config.json";
    match fs::read_to_string(config_path) {
        Ok(content) => {
            match serde_json::from_str::<SamplerConfig>(&content) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Failed to parse config file: {}, using defaults", e);
                    SamplerConfig::default()
                }
            }
        }
        Err(_) => {
            // Config file doesn't exist, use defaults
            SamplerConfig::default()
        }
    }
}

// Helper function to add a model path to history
pub fn add_to_model_history(model_path: &str) {
    let config_path = "assets/config.json";

    // Load current config
    let mut config = load_config();

    // Remove the path if it already exists (to move it to the front)
    config.model_history.retain(|p| p != model_path);

    // Add to the front of the list
    config.model_history.insert(0, model_path.to_string());

    // Keep only the last 10 paths
    if config.model_history.len() > 10 {
        config.model_history.truncate(10);
    }

    // Save the updated config
    let _ = fs::create_dir_all("assets");
    if let Ok(json) = serde_json::to_string_pretty(&config) {
        let _ = fs::write(config_path, json);
    }
}
