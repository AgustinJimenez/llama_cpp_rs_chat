//! GBNF grammar for constraining tool call JSON output.
//!
//! Uses llama.cpp's lazy grammar feature: grammar is only enforced after
//! a trigger word is detected (e.g. `{"name"`), allowing free-form text
//! before the tool call.

use llama_cpp_2::{model::LlamaModel, sampling::LlamaSampler};

/// GBNF grammar that constrains output to valid JSON tool call format.
/// Supports the standard `{"name": "...", "arguments": {...}}` format.
const TOOL_CALL_GBNF: &str = r#"
root        ::= tool-call
tool-call   ::= "{" ws "\"name\"" ws ":" ws string "," ws "\"arguments\"" ws ":" ws object ws "}"
object      ::= "{" ws "}" | "{" ws members ws "}"
members     ::= pair ("," ws pair)*
pair        ::= string ws ":" ws value
array       ::= "[" ws "]" | "[" ws values ws "]"
values      ::= value ("," ws value)*
value       ::= string | number | object | array | "true" | "false" | "null"
string      ::= "\"" chars "\""
chars       ::= char*
char        ::= [^"\\] | "\\" escape
escape      ::= ["\\nrtbf/] | "u" hex hex hex hex
hex         ::= [0-9a-fA-F]
number      ::= "-"? int frac? exp?
int         ::= "0" | [1-9] [0-9]*
frac        ::= "." [0-9]+
exp         ::= [eE] [+-]? [0-9]+
ws          ::= [ \t\n]*
"#;

/// Trigger words that activate the grammar constraint.
/// When the model starts producing any of these, the grammar kicks in.
const TRIGGER_WORDS: &[&[u8]] = &[
    b"{\"name\"",    // Standard JSON tool call
    b"{ \"name\"",   // With space after brace
];

/// Create a lazy grammar sampler for tool call JSON constraints.
///
/// Returns `None` if grammar creation fails (non-fatal — generation
/// continues without grammar constraints).
pub fn create_tool_grammar_sampler(model: &LlamaModel) -> Option<LlamaSampler> {
    match LlamaSampler::grammar_lazy(
        model,
        TOOL_CALL_GBNF,
        "root",
        TRIGGER_WORDS.iter().copied(),
        &[], // no trigger tokens
    ) {
        Ok(sampler) => {
            eprintln!("[GRAMMAR] Tool call grammar sampler created (lazy mode)");
            Some(sampler)
        }
        Err(e) => {
            eprintln!("[GRAMMAR] Failed to create tool grammar (non-fatal): {e:?}");
            None
        }
    }
}
