mod commands;
mod error;
mod providers;
mod types;

use std::time::Duration;

pub use error::{CommandError, ErrorCode};
pub use types::{
    ChatMessage, ChatRequest, ChatResponse, ChatRole, ConnectionTestRequest,
    ConnectionTestResponse, ProviderCapabilities, ProviderCredentials, ProviderKind,
    ProviderProtocol, SecretString, TokenUsage,
};

pub(crate) struct AppState {
    pub(crate) http: reqwest::Client,
}

impl AppState {
    fn new() -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("Kiln/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("the platform TLS and HTTP client should initialize");

        Self { http }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_provider_capabilities,
            commands::test_connection,
            commands::send_chat_request,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Kiln desktop application");
}
