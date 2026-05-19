//! Shared primitives and submodule declarations for tool category files.

use serde_json::Value;

// ─── Shared primitives (used by all category modules) ──────────────────────

/// A compact tool definition.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub params: Params,
    pub required: &'static [&'static str],
}

/// Parameters — either all simple scalars, or a mix with raw JSON values.
#[allow(dead_code)]
pub enum Params {
    /// All parameters are simple scalar types.
    Simple(&'static [ParamDef]),
    /// Some parameters need raw JSON (arrays, objects with nested schemas).
    Mixed(&'static [ParamDef], &'static [RawParam]),
}

/// A compact parameter definition for simple scalar types.
pub struct ParamDef {
    pub name: &'static str,
    pub param_type: &'static str, // "string", "integer", "boolean", "number"
    pub description: &'static str,
}

/// A parameter that needs a full JSON value (for arrays, objects, etc.).
#[allow(dead_code)]
pub struct RawParam {
    pub name: &'static str,
    pub build: fn() -> Value,
}

/// Shorthand constructor for ParamDef (used in all category modules).
pub const fn p(
    name: &'static str,
    param_type: &'static str,
    description: &'static str,
) -> ParamDef {
    ParamDef { name, param_type, description }
}

/// Verification params shared across desktop input tools.
pub static VERIFY_PARAMS: &[ParamDef] = &[
    p("verify_screen_change", "boolean", "If true, verify that the screen visibly changed after the action before returning."),
    p("verify_threshold_pct", "number", "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)."),
    p("verify_timeout_ms", "integer", "Maximum time to wait for a visible change when verification is enabled (default: 1200)."),
    p("verify_poll_ms", "integer", "Polling interval for verification screenshots (default: 150)."),
    p("verify_x", "integer", "Optional absolute X for a custom verification region."),
    p("verify_y", "integer", "Optional absolute Y for a custom verification region."),
    p("verify_width", "integer", "Optional width for a custom verification region."),
    p("verify_height", "integer", "Optional height for a custom verification region."),
    p("verify_text", "string", "After action, OCR the verification region and confirm this text appears. Enables verify_screen_change automatically."),
];

impl ToolDef {
    pub fn to_json(&self) -> Value {
        use serde_json::json;
        let mut properties = serde_json::Map::new();
        match &self.params {
            Params::Simple(defs) => {
                for p in *defs {
                    properties.insert(
                        p.name.to_string(),
                        json!({ "type": p.param_type, "description": p.description }),
                    );
                }
            }
            Params::Mixed(defs, raws) => {
                for p in *defs {
                    properties.insert(
                        p.name.to_string(),
                        json!({ "type": p.param_type, "description": p.description }),
                    );
                }
                for r in *raws {
                    properties.insert(r.name.to_string(), (r.build)());
                }
            }
        }
        let required: Vec<&str> = self.required.to_vec();
        json!({
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required,
            }
        })
    }
}

// ─── Category modules ───────────────────────────────────────────────────────

pub mod agent_tools;
pub mod browser_tools;
pub mod clipboard_tools;
pub mod file_tools;
pub mod input_tools;
pub mod screenshot_tools;
pub mod system_tools;
pub mod ui_automation_tools;
pub mod window_tools;
