mod commands;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub use kiln_core::{
    ChatMessage, ChatRequest, ChatResponse, ChatRole, ChatStreamEvent, CommandError,
    ConnectionTestRequest, ConnectionTestResponse, CredentialBackendKind, CredentialProfileRef,
    CredentialSaveRequest, ErrorCode, EventEnvelope, ProviderCapabilities,
    ProviderCredentialProfile, ProviderCredentials, ProviderKind, ProviderProtocol, SecretString,
    StreamId, TokenUsage,
};
use kiln_platform::{CancellationToken, OsCredentialStore, SystemClock};
use kiln_providers::ProviderService;
use kiln_storage::SqliteEventStore;
use kiln_workspace::{GitRepositoryInspector, WorkspaceToolService};
use tauri::Manager;

pub(crate) struct AppState {
    pub(crate) providers: ProviderService,
    pub(crate) credentials: OsCredentialStore,
    pub(crate) storage: SqliteEventStore,
    pub(crate) active_turns: TurnCancellationRegistry,
    pub(crate) repositories: GitRepositoryInspector,
    pub(crate) workspace_tools: WorkspaceToolService,
    pub(crate) clock: SystemClock,
}

impl AppState {
    fn new(storage: SqliteEventStore) -> Self {
        Self {
            providers: ProviderService::new(),
            credentials: OsCredentialStore::new(),
            storage,
            active_turns: TurnCancellationRegistry::default(),
            repositories: GitRepositoryInspector::default(),
            workspace_tools: WorkspaceToolService::default(),
            clock: SystemClock,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TurnCancellationRegistry {
    inner: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl TurnCancellationRegistry {
    pub(crate) fn start(&self, turn_id: String) -> Result<CancellationToken, ()> {
        let mut turns = self.inner.lock().expect("turn registry poisoned");
        if turns.contains_key(&turn_id) {
            return Err(());
        }
        let cancellation = CancellationToken::default();
        turns.insert(turn_id, cancellation.clone());
        Ok(cancellation)
    }

    pub(crate) fn cancel(&self, turn_id: &str) -> bool {
        let turns = self.inner.lock().expect("turn registry poisoned");
        match turns.get(turn_id) {
            Some(cancellation) => {
                cancellation.cancel();
                true
            }
            None => false,
        }
    }

    pub(crate) fn finish(&self, turn_id: &str) {
        self.inner
            .lock()
            .expect("turn registry poisoned")
            .remove(turn_id);
    }

    pub(crate) fn token(&self, turn_id: &str) -> Option<CancellationToken> {
        self.inner
            .lock()
            .expect("turn registry poisoned")
            .get(turn_id)
            .cloned()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|application| {
            let data_dir = application.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let database_path = data_dir.join("kiln.db");
            let storage =
                tauri::async_runtime::block_on(SqliteEventStore::connect_path(database_path))?;
            application.manage(AppState::new(storage));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_provider_capabilities,
            commands::list_provider_credentials,
            commands::save_provider_credential,
            commands::delete_provider_credential,
            commands::test_connection,
            commands::send_chat_request,
            commands::start_chat_stream,
            commands::cancel_turn,
            commands::append_application_events,
            commands::load_application_events,
            commands::open_repository,
            commands::list_remembered_projects,
            commands::execute_repository_tool,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Kiln desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_cancellation_registry_rejects_duplicates_and_cleans_up() {
        let registry = TurnCancellationRegistry::default();
        let token = registry.start("turn-1".to_owned()).unwrap();
        assert!(registry.start("turn-1".to_owned()).is_err());
        assert!(registry.cancel("turn-1"));
        assert!(token.is_cancelled());

        registry.finish("turn-1");
        assert!(!registry.cancel("turn-1"));
        assert!(registry.start("turn-1".to_owned()).is_ok());
    }
}
