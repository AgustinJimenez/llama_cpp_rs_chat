// Re-export route handler modules from the llama-chat-web crate

#[allow(unused_imports)]
pub mod chat {
    pub use llama_chat_web::routes::chat::*;
}
#[allow(unused_imports)]
pub mod config {
    pub use llama_chat_web::routes::config::*;
}
#[allow(unused_imports)]
pub mod conversation {
    pub use llama_chat_web::routes::conversation::*;
}
#[allow(unused_imports)]
pub mod download {
    pub use llama_chat_web::routes::download::*;
}
#[allow(unused_imports)]
pub mod files {
    pub use llama_chat_web::routes::files::*;
}
#[allow(unused_imports)]
pub mod frontend_logs {
    pub use llama_chat_web::routes::frontend_logs::*;
}
#[allow(unused_imports)]
pub mod health {
    pub use llama_chat_web::routes::health::*;
}
#[allow(unused_imports)]
pub mod hub {
    pub use llama_chat_web::routes::hub::*;
}
#[allow(unused_imports)]
pub mod model {
    pub use llama_chat_web::routes::model::*;
}
#[allow(unused_imports)]
pub mod static_files {
    pub use llama_chat_web::routes::static_files::*;
}
#[allow(unused_imports)]
pub mod status {
    pub use llama_chat_web::routes::status::*;
}
#[allow(unused_imports)]
pub mod system {
    pub use llama_chat_web::routes::system::*;
}
#[allow(unused_imports)]
pub mod mcp {
    pub use llama_chat_web::routes::mcp::*;
}
#[allow(unused_imports)]
pub mod providers {
    pub use llama_chat_web::routes::providers::*;
}
#[allow(unused_imports)]
pub mod tools {
    pub use llama_chat_web::routes::tools::*;
}
#[allow(unused_imports)]
pub mod app_errors {
    pub use llama_chat_web::routes::app_errors::*;
}
