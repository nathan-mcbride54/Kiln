use std::path::PathBuf;

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::AppState;
use kiln_core::{
    ApplicationEvent, ChatRequest, ChatResponse, ChatStreamEvent, CommandError,
    ConnectionTestRequest, ConnectionTestResponse, CredentialProfileRef, CredentialSaveRequest,
    ErrorCode, EventEnvelope, EventId, OpenProjectRequest, ProjectSnapshot, ProviderCapabilities,
    ProviderCredentialProfile, ProviderCredentials, ProviderKind, RememberedProject,
    RepositoryToolExecution, RepositoryToolRequest, StreamId, APPLICATION_CONTRACT_VERSION,
};
use kiln_platform::{CancellationToken, Clock, CredentialStoreError, OsCredentialStore};
use kiln_workspace::{RepositoryError, WorkspaceToolError};

#[tauri::command]
pub fn list_provider_capabilities(state: State<'_, AppState>) -> Vec<ProviderCapabilities> {
    state.providers.capabilities()
}

#[tauri::command]
pub async fn list_provider_credentials(
    state: State<'_, AppState>,
) -> Result<Vec<ProviderCredentialProfile>, CommandError> {
    let store = state.credentials.clone();
    tauri::async_runtime::spawn_blocking(move || store.list_profiles())
        .await
        .map_err(|_| credential_task_error(None))?
        .map_err(|error| credential_error(error, None))
}

#[tauri::command]
pub async fn save_provider_credential(
    state: State<'_, AppState>,
    request: CredentialSaveRequest,
) -> Result<ProviderCredentialProfile, CommandError> {
    let store = state.credentials.clone();
    let provider = request.provider;
    let origin = state
        .providers
        .credential_origin(provider, request.base_url.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || store.save(provider, &origin, &request.secret))
        .await
        .map_err(|_| credential_task_error(Some(provider)))?
        .map_err(|error| credential_error(error, Some(provider)))
}

#[tauri::command]
pub async fn delete_provider_credential(
    state: State<'_, AppState>,
    provider: ProviderKind,
    credential_ref: CredentialProfileRef,
) -> Result<(), CommandError> {
    let store = state.credentials.clone();
    tauri::async_runtime::spawn_blocking(move || store.delete(provider, &credential_ref))
        .await
        .map_err(|_| credential_task_error(Some(provider)))?
        .map_err(|error| credential_error(error, Some(provider)))
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    request: ConnectionTestRequest,
) -> Result<ConnectionTestResponse, CommandError> {
    let request =
        resolve_connection_credentials(state.credentials.clone(), state.providers.clone(), request)
            .await?;
    state.providers.test_connection(&request).await
}

#[tauri::command]
pub async fn send_chat_request(
    state: State<'_, AppState>,
    request: ChatRequest,
) -> Result<ChatResponse, CommandError> {
    let request =
        resolve_chat_credentials(state.credentials.clone(), state.providers.clone(), request)
            .await?;
    state.providers.send_chat(&request).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum DesktopStreamEvent {
    Provider { event: ChatStreamEvent },
    Error { error: CommandError },
}

#[tauri::command]
pub async fn start_chat_stream(
    state: State<'_, AppState>,
    turn_id: String,
    request: ChatRequest,
    on_event: Channel<DesktopStreamEvent>,
) -> Result<(), CommandError> {
    if turn_id.trim().is_empty() {
        return Err(invalid_turn("The turn identifier cannot be blank."));
    }
    let request =
        resolve_chat_credentials(state.credentials.clone(), state.providers.clone(), request)
            .await?;
    let cancellation = state
        .active_turns
        .start(turn_id.clone())
        .map_err(|_| invalid_turn("This turn is already active."))?;
    let mut receiver = match state.providers.stream_chat(request, cancellation.clone()) {
        Ok(receiver) => receiver,
        Err(error) => {
            state.active_turns.finish(&turn_id);
            return Err(error);
        }
    };
    let active_turns = state.active_turns.clone();

    tauri::async_runtime::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let channel_event = match event {
                Ok(event) => DesktopStreamEvent::Provider { event },
                Err(error) => DesktopStreamEvent::Error { error },
            };
            if on_event.send(channel_event).is_err() {
                cancellation.cancel();
                break;
            }
        }
        active_turns.finish(&turn_id);
    });
    Ok(())
}

async fn resolve_connection_credentials(
    store: OsCredentialStore,
    providers: kiln_providers::ProviderService,
    mut request: ConnectionTestRequest,
) -> Result<ConnectionTestRequest, CommandError> {
    let Some(credential_ref) = request.credential_ref.clone() else {
        return Ok(request);
    };
    let provider = request.provider;
    let origin = providers.credential_origin(provider, request.base_url.as_deref())?;
    let secret = tauri::async_runtime::spawn_blocking(move || {
        store.resolve(provider, &origin, &credential_ref)
    })
    .await
    .map_err(|_| credential_task_error(Some(provider)))?
    .map_err(|error| credential_error(error, Some(provider)))?;
    request.credentials = ProviderCredentials {
        api_key: Some(secret),
        ..ProviderCredentials::default()
    };
    Ok(request)
}

async fn resolve_chat_credentials(
    store: OsCredentialStore,
    providers: kiln_providers::ProviderService,
    mut request: ChatRequest,
) -> Result<ChatRequest, CommandError> {
    let Some(credential_ref) = request.credential_ref.clone() else {
        return Ok(request);
    };
    let provider = request.provider;
    let origin = providers.credential_origin(provider, request.base_url.as_deref())?;
    let secret = tauri::async_runtime::spawn_blocking(move || {
        store.resolve(provider, &origin, &credential_ref)
    })
    .await
    .map_err(|_| credential_task_error(Some(provider)))?
    .map_err(|error| credential_error(error, Some(provider)))?;
    request.credentials = ProviderCredentials {
        api_key: Some(secret),
        ..ProviderCredentials::default()
    };
    Ok(request)
}

fn credential_error(error: CredentialStoreError, provider: Option<ProviderKind>) -> CommandError {
    CommandError {
        code: match error {
            CredentialStoreError::BlankSecret => ErrorCode::InvalidRequest,
            _ => ErrorCode::CredentialFailure,
        },
        message: error.to_string(),
        provider,
        status: None,
        retryable: matches!(
            error,
            CredentialStoreError::Unavailable | CredentialStoreError::ReferenceGeneration
        ),
    }
}

fn credential_task_error(provider: Option<ProviderKind>) -> CommandError {
    CommandError {
        code: ErrorCode::CredentialFailure,
        message: "The operating-system credential task could not be completed.".to_owned(),
        provider,
        status: None,
        retryable: true,
    }
}

#[tauri::command]
pub fn cancel_turn(state: State<'_, AppState>, turn_id: String) -> bool {
    state.active_turns.cancel(&turn_id)
}

#[tauri::command]
pub async fn append_application_events(
    state: State<'_, AppState>,
    events: Vec<EventEnvelope>,
) -> Result<(), CommandError> {
    state.storage.append(&events).await.map_err(storage_error)
}

#[tauri::command]
pub async fn load_application_events(
    state: State<'_, AppState>,
    stream_id: String,
) -> Result<Vec<EventEnvelope>, CommandError> {
    let stream_id = StreamId::new(stream_id).map_err(|_| CommandError {
        code: ErrorCode::InvalidRequest,
        message: "The event stream identifier is invalid.".to_owned(),
        provider: None,
        status: None,
        retryable: false,
    })?;
    state
        .storage
        .load_stream(&stream_id)
        .await
        .map_err(storage_error)
}

#[tauri::command]
pub async fn open_repository(
    state: State<'_, AppState>,
    request: OpenProjectRequest,
) -> Result<ProjectSnapshot, CommandError> {
    request.validate().map_err(|_| {
        invalid_project("Enter a repository path and valid project defaults.", false)
    })?;
    let inspector = state.repositories.clone();
    let path = PathBuf::from(request.path);
    let defaults = request.defaults;
    let project = tauri::async_runtime::spawn_blocking(move || inspector.inspect(path, defaults))
        .await
        .map_err(|_| invalid_project("Kiln could not finish inspecting this repository.", true))?
        .map_err(repository_error)?;

    state
        .workspace_tools
        .register(&project)
        .map_err(workspace_tool_error)?;
    remember_project(&state.storage, &project, state.clock.now_unix_ms()).await?;
    Ok(project)
}

#[tauri::command]
pub async fn list_remembered_projects(
    state: State<'_, AppState>,
) -> Result<Vec<RememberedProject>, CommandError> {
    let events = state
        .storage
        .load_latest_events_by_type("project_opened", 12)
        .await
        .map_err(storage_error)?;
    let mut remembered = Vec::with_capacity(events.len());

    for event in events {
        let Some(stored) = project_from_event(&event.payload) else {
            continue;
        };
        let inspector = state.repositories.clone();
        let candidate = stored.clone();
        let refreshed = tauri::async_runtime::spawn_blocking(move || inspector.refresh(&candidate))
            .await
            .map_err(|_| {
                invalid_project("Kiln could not finish refreshing recent projects.", true)
            })?;
        match refreshed {
            Ok(project) => match state.workspace_tools.register(&project) {
                Ok(()) => remembered.push(RememberedProject {
                    project,
                    last_opened_at_ms: event.occurred_at_ms,
                    available: true,
                    unavailable_reason: None,
                }),
                Err(error) => remembered.push(RememberedProject {
                    project: stored,
                    last_opened_at_ms: event.occurred_at_ms,
                    available: false,
                    unavailable_reason: Some(error.user_message().to_owned()),
                }),
            },
            Err(error) => remembered.push(RememberedProject {
                project: stored,
                last_opened_at_ms: event.occurred_at_ms,
                available: false,
                unavailable_reason: Some(error.user_message().to_owned()),
            }),
        }
    }
    Ok(remembered)
}

#[tauri::command]
pub async fn execute_repository_tool(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    tool_call_id: String,
    request: RepositoryToolRequest,
    turn_id: Option<String>,
) -> Result<RepositoryToolExecution, CommandError> {
    let cancellation = match turn_id {
        Some(turn_id) => state.active_turns.token(&turn_id).ok_or(CommandError {
            code: ErrorCode::Cancelled,
            message: "This turn is no longer active.".to_owned(),
            provider: None,
            status: None,
            retryable: false,
        })?,
        None => CancellationToken::default(),
    };
    let tools = state.workspace_tools.clone();
    let approval_path = match &request {
        RepositoryToolRequest::WriteFile(request) => Some(request.path.clone()),
        _ => None,
    };
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(path) = approval_path {
            let approved = app
                .dialog()
                .message(format!(
                    "Kiln wants to replace the UTF-8 contents of:\n\n{path}\n\nReview the resulting diff before accepting the task."
                ))
                .title("Approve one workspace edit")
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Apply edit".to_owned(),
                    "Cancel".to_owned(),
                ))
                .blocking_show();
            if !approved {
                return Err(WorkspaceToolError::ApprovalDeclined);
            }
            tools.approve_once(&project_id, &tool_call_id, &request)?;
        }
        tools
            .execute(&project_id, &tool_call_id, request, &cancellation)
            .map(RepositoryToolExecution::new)
    })
    .await
    .map_err(|_| CommandError {
        code: ErrorCode::ProviderFailure,
        message: "Kiln could not finish the repository tool.".to_owned(),
        provider: None,
        status: None,
        retryable: true,
    })?
    .map_err(workspace_tool_error)
}

async fn remember_project(
    storage: &kiln_storage::SqliteEventStore,
    project: &ProjectSnapshot,
    occurred_at_ms: u64,
) -> Result<(), CommandError> {
    let stream_id = StreamId::new(format!("project:{}", project.project_id)).map_err(|_| {
        invalid_project(
            "Kiln could not create a safe identity for this repository.",
            false,
        )
    })?;
    let sequence = storage
        .load_stream(&stream_id)
        .await
        .map_err(storage_error)?
        .last()
        .map_or(1, |event| event.sequence.saturating_add(1));
    let causation_id = format!("command:open-repository:{}:{sequence}", project.project_id);
    let events = [
        EventEnvelope {
            schema_version: APPLICATION_CONTRACT_VERSION,
            event_id: EventId::new(format!("{}:{sequence}", stream_id.as_str())).map_err(|_| {
                invalid_project("Kiln could not record this repository safely.", false)
            })?,
            stream_id: stream_id.clone(),
            task_id: None,
            sequence,
            occurred_at_ms,
            causation_id: Some(causation_id.clone()),
            correlation_id: None,
            payload: ApplicationEvent::ProjectOpened {
                project_id: project.project_id.clone(),
                root: project.root.clone(),
                display_name: project.display_name.clone(),
                branch: project.branch.clone(),
                head: project.head.clone(),
                status: project.status.clone(),
                defaults: project.defaults.clone(),
            },
        },
        EventEnvelope {
            schema_version: APPLICATION_CONTRACT_VERSION,
            event_id: EventId::new(format!(
                "{}:{}",
                stream_id.as_str(),
                sequence.saturating_add(1)
            ))
            .map_err(|_| invalid_project("Kiln could not record this workspace safely.", false))?,
            stream_id,
            task_id: None,
            sequence: sequence.saturating_add(1),
            occurred_at_ms,
            causation_id: Some(causation_id),
            correlation_id: None,
            payload: ApplicationEvent::WorkspaceReady {
                workspace_id: format!("workspace:direct:{}", project.project_id),
                project_id: project.project_id.clone(),
                path: project.root.clone(),
                isolated: false,
            },
        },
    ];
    storage.append(&events).await.map_err(storage_error)
}

fn project_from_event(event: &ApplicationEvent) -> Option<ProjectSnapshot> {
    match event {
        ApplicationEvent::ProjectOpened {
            project_id,
            root,
            display_name,
            branch,
            head,
            status,
            defaults,
        } => Some(ProjectSnapshot {
            project_id: project_id.clone(),
            display_name: display_name.clone(),
            root: root.clone(),
            branch: branch.clone(),
            head: head.clone(),
            status: status.clone(),
            defaults: defaults.clone(),
        }),
        _ => None,
    }
}

fn storage_error(error: kiln_storage::StorageError) -> CommandError {
    let retryable = matches!(error, kiln_storage::StorageError::Database(_));
    CommandError {
        code: ErrorCode::StorageFailure,
        message: "Kiln could not update its local history.".to_owned(),
        provider: None,
        status: None,
        retryable,
    }
}

fn repository_error(error: RepositoryError) -> CommandError {
    let retryable = matches!(
        error,
        RepositoryError::GitUnavailable
            | RepositoryError::GitCommandFailed
            | RepositoryError::SelectionInaccessible(_)
            | RepositoryError::InspectionTimedOut
            | RepositoryError::GitOutputTooLarge
    );
    let code = if matches!(error, RepositoryError::GitUnavailable) {
        ErrorCode::InvalidConfiguration
    } else {
        ErrorCode::InvalidRequest
    };
    CommandError {
        code,
        message: error.user_message().to_owned(),
        provider: None,
        status: None,
        retryable,
    }
}

fn workspace_tool_error(error: WorkspaceToolError) -> CommandError {
    let code = match error {
        WorkspaceToolError::ApprovalDeclined => ErrorCode::PermissionDenied,
        WorkspaceToolError::Cancelled => ErrorCode::Cancelled,
        _ => ErrorCode::InvalidRequest,
    };
    let retryable = matches!(
        error,
        WorkspaceToolError::Unavailable
            | WorkspaceToolError::RepositoryIndex(_)
            | WorkspaceToolError::Io(_)
    );
    CommandError {
        code,
        message: error.user_message().to_owned(),
        provider: None,
        status: None,
        retryable,
    }
}

fn invalid_project(message: &str, retryable: bool) -> CommandError {
    CommandError {
        code: ErrorCode::InvalidRequest,
        message: message.to_owned(),
        provider: None,
        status: None,
        retryable,
    }
}

fn invalid_turn(message: &str) -> CommandError {
    CommandError {
        code: ErrorCode::InvalidRequest,
        message: message.to_owned(),
        provider: None,
        status: None,
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_errors_do_not_expose_payload_details() {
        let error = storage_error(kiln_storage::StorageError::SensitiveData(
            "payload.data.apiKey".to_owned(),
        ));

        assert_eq!(error.code, ErrorCode::StorageFailure);
        assert!(!error.message.contains("apiKey"));
        assert!(!error.retryable);
    }

    #[test]
    fn approval_declines_and_turn_cancellation_have_distinct_codes() {
        let declined = workspace_tool_error(WorkspaceToolError::ApprovalDeclined);
        let cancelled = workspace_tool_error(WorkspaceToolError::Cancelled);

        assert_eq!(declined.code, ErrorCode::PermissionDenied);
        assert_eq!(cancelled.code, ErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn remembered_project_events_contain_no_credential_fields() {
        let store = kiln_storage::SqliteEventStore::in_memory().await.unwrap();
        let project = ProjectSnapshot {
            project_id: "project-safe".to_owned(),
            display_name: "safe".to_owned(),
            root: "/work/safe".to_owned(),
            branch: Some("main".to_owned()),
            head: None,
            status: kiln_core::RepositoryStatus::default(),
            defaults: kiln_core::ProjectDefaults {
                provider: Some(kiln_core::ProviderKind::OpenAi),
                model: Some("gpt-5".to_owned()),
                verification_profile: None,
            },
        };

        remember_project(&store, &project, 42).await.unwrap();
        let stored = store
            .load_latest_events_by_type("project_opened", 12)
            .await
            .unwrap();
        let json = serde_json::to_string(&stored).unwrap().to_ascii_lowercase();
        assert!(!json.contains("credential"));
        assert!(!json.contains("apikey"));
        assert!(!json.contains("authorization"));
    }
}
