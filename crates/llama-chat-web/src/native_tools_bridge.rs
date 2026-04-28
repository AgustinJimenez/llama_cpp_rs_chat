//! Bridge between the tools crate and the web crate's skill system.
//!
//! Provides `make_dispatch_context()` which connects the tools crate's
//! dispatch machinery to the web crate's skill discovery and the engine
//! crate's tool catalog.

/// Build a `DispatchContext` that connects the tools crate to the
/// skill system and tool catalog.
pub fn make_dispatch_context() -> llama_chat_tools::DispatchContext<'static> {
    llama_chat_tools::DispatchContext {
        get_tool_catalog: Some(&|category| {
            llama_chat_engine::jinja_templates::get_tool_catalog(category)
        }),
        get_tool_schema: Some(&|tool_name| {
            llama_chat_engine::jinja_templates::get_tool_schema(tool_name)
        }),
        discover_skills: Some(&|cwd| {
            crate::skills::discover_skills(cwd)
                .into_iter()
                .map(|s| llama_chat_tools::SkillInfo {
                    name: s.name,
                    description: s.description,
                    content: s.content,
                })
                .collect()
        }),
        get_skill: Some(&|cwd, name| {
            crate::skills::get_skill(cwd, name).map(|s| llama_chat_tools::SkillInfo {
                name: s.name,
                description: s.description,
                content: s.content,
            })
        }),
    }
}
