pub use super::openai_compat_types::{ProviderPreset, PROVIDER_PRESETS};
pub use generate::generate;
pub use resolve::{
    fetch_models, get_preset, is_openai_compat, resolve_api_key, resolve_base_url,
    resolve_custom_field,
};

mod db;
mod generate;
mod resolve;
