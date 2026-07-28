use tauri::State;

use crate::{
    error::CommandError,
    providers,
    types::{
        ChatRequest, ChatResponse, ConnectionTestRequest, ConnectionTestResponse,
        ProviderCapabilities,
    },
    AppState,
};

#[tauri::command]
pub fn list_provider_capabilities() -> Vec<ProviderCapabilities> {
    providers::all_capabilities()
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    request: ConnectionTestRequest,
) -> Result<ConnectionTestResponse, CommandError> {
    let provider = request.provider;
    let http = state.http.clone();

    providers::test_connection(&http, &request)
        .await
        .map_err(|error| error.into_command(provider))
}

#[tauri::command]
pub async fn send_chat_request(
    state: State<'_, AppState>,
    request: ChatRequest,
) -> Result<ChatResponse, CommandError> {
    let provider = request.provider;
    let http = state.http.clone();

    providers::send_chat(&http, &request)
        .await
        .map_err(|error| error.into_command(provider))
}
