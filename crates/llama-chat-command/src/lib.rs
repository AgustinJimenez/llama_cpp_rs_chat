// Shell command execution, output sanitization, and background process management.

#[macro_use]
extern crate llama_chat_types;

mod shell_env;
mod parsing;
mod execution;
mod output;
pub mod background;
mod utils;

#[allow(unused_imports)]
pub use shell_env::get_shell_env;
#[allow(unused_imports)]
pub use parsing::parse_command_with_quotes;
pub use execution::{
    execute_command,
    execute_command_streaming,
    execute_command_streaming_with_timeout,
    kill_process_tree,
};
#[cfg(windows)]
pub use execution::enriched_windows_path;
pub use output::{strip_ansi_codes, sanitize_command_output};
#[allow(unused_imports)]
pub use output::truncate_command_output;
pub use utils::silent_command;
