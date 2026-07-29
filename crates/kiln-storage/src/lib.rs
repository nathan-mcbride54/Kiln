//! Transactional SQLite storage for Kiln's immutable application events.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use kiln_core::{
    ApplicationEvent, ContractError, EventEnvelope, EventId, EventSequence, SensitiveDataRedactor,
    StreamId, TaskId,
};
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow},
    Row, Sqlite, SqlitePool, Transaction,
};
use thiserror::Error;

pub const STORAGE_SCHEMA_VERSION: i64 = 1;

const MIGRATION_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at_ms INTEGER NOT NULL
)
"#;

const EVENT_TABLE: &str = r#"
CREATE TABLE event_log (
    stream_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    event_id TEXT NOT NULL UNIQUE,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    task_id TEXT,
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    causation_id TEXT,
    correlation_id TEXT,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (stream_id, sequence)
)
"#;

const EVENT_TASK_INDEX: &str =
    "CREATE INDEX event_log_task_sequence ON event_log(task_id, sequence)";
const EVENT_TYPE_INDEX: &str =
    "CREATE INDEX event_log_type_time ON event_log(event_type, occurred_at_ms)";

#[derive(Clone)]
pub struct SqliteEventStore {
    pool: SqlitePool,
}

impl SqliteEventStore {
    pub async fn connect(database_url: &str) -> Result<Self, StorageError> {
        let pool = SqlitePoolOptions::new()
            // H0 intentionally uses one writer connection. This makes append
            // order explicit; supervised read pools arrive with H2.
            .max_connections(1)
            .connect(database_url)
            .await?;
        Self::initialize(pool).await
    }

    pub async fn connect_path(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Self::initialize(pool).await
    }

    async fn initialize(pool: SqlitePool) -> Result<Self, StorageError> {
        let store = Self { pool };
        store.configure().await?;
        store.ensure_migration_table().await?;
        store.migrate_to_latest().await?;
        Ok(store)
    }

    pub async fn in_memory() -> Result<Self, StorageError> {
        Self::connect("sqlite::memory:").await
    }

    pub async fn schema_version(&self) -> Result<i64, StorageError> {
        let version =
            sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(version) FROM schema_migrations")
                .fetch_one(&self.pool)
                .await?;
        Ok(version.unwrap_or(0))
    }

    /// Appends a causally ordered batch for exactly one stream.
    ///
    /// Validation completes before the transaction starts. The transaction
    /// also checks the durable tail so duplicate or skipped sequence numbers
    /// cannot partially commit.
    pub async fn append(&self, events: &[EventEnvelope]) -> Result<(), StorageError> {
        if events.is_empty() {
            return Ok(());
        }

        validate_batch(events)?;
        let stream_id = events[0].stream_id.as_str();
        let mut transaction = self.pool.begin().await?;
        let expected = next_sequence(&mut transaction, stream_id).await?;
        if events[0].sequence != expected {
            return Err(StorageError::UnexpectedSequence {
                expected,
                found: events[0].sequence,
            });
        }

        for event in events {
            let payload_json = serde_json::to_string(&event.payload)?;
            let occurred_at_ms = i64::try_from(event.occurred_at_ms)
                .map_err(|_| StorageError::TimestampOutOfRange(event.occurred_at_ms))?;
            let sequence = i64::try_from(event.sequence)
                .map_err(|_| StorageError::SequenceOutOfRange(event.sequence))?;

            sqlx::query(
                r#"
                INSERT INTO event_log (
                    stream_id, sequence, event_id, schema_version, task_id,
                    occurred_at_ms, causation_id, correlation_id, event_type,
                    payload_json
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(event.stream_id.as_str())
            .bind(sequence)
            .bind(event.event_id.as_str())
            .bind(i64::from(event.schema_version))
            .bind(event.task_id.as_ref().map(TaskId::as_str))
            .bind(occurred_at_ms)
            .bind(event.causation_id.as_deref())
            .bind(event.correlation_id.as_deref())
            .bind(event.payload.kind())
            .bind(payload_json)
            .execute(&mut *transaction)
            .await?;
        }

        transaction.commit().await?;
        Ok(())
    }

    pub async fn load_stream(
        &self,
        stream_id: &StreamId,
    ) -> Result<Vec<EventEnvelope>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT
                stream_id, sequence, event_id, schema_version, task_id,
                occurred_at_ms, causation_id, correlation_id, payload_json
            FROM event_log
            WHERE stream_id = ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(stream_id.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::with_capacity(rows.len());
        let mut sequence_validator = EventSequence::new(stream_id.clone());
        for row in rows {
            let event = decode_event(row)?;
            sequence_validator.observe(&event)?;
            events.push(event);
        }
        Ok(events)
    }

    /// Loads the newest matching event from each stream, ordered by most
    /// recently observed first. This supports rebuildable recent-project views
    /// without introducing a mutable side table.
    pub async fn load_latest_events_by_type(
        &self,
        event_type: &str,
        limit: u32,
    ) -> Result<Vec<EventEnvelope>, StorageError> {
        if event_type.trim().is_empty() || limit == 0 || limit > 100 {
            return Err(StorageError::InvalidQuery);
        }
        let rows = sqlx::query(
            r#"
            SELECT
                event.stream_id, event.sequence, event.event_id,
                event.schema_version, event.task_id, event.occurred_at_ms,
                event.causation_id, event.correlation_id, event.payload_json
            FROM event_log AS event
            WHERE event.event_type = ?
              AND event.sequence = (
                SELECT MAX(candidate.sequence)
                FROM event_log AS candidate
                WHERE candidate.stream_id = event.stream_id
                  AND candidate.event_type = event.event_type
              )
            ORDER BY event.occurred_at_ms DESC, event.stream_id ASC
            LIMIT ?
            "#,
        )
        .bind(event_type)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let event = decode_event(row)?;
                event.validate()?;
                Ok(event)
            })
            .collect()
    }

    pub async fn event_count(&self) -> Result<i64, StorageError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM event_log")
            .fetch_one(&self.pool)
            .await?)
    }

    async fn configure(&self) -> Result<(), StorageError> {
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA busy_timeout = 5000")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn ensure_migration_table(&self) -> Result<(), StorageError> {
        sqlx::query(MIGRATION_TABLE).execute(&self.pool).await?;
        Ok(())
    }

    async fn migrate_to_latest(&self) -> Result<(), StorageError> {
        match self.schema_version().await? {
            0 => self.apply_initial_migration().await,
            STORAGE_SCHEMA_VERSION => Ok(()),
            version => Err(StorageError::UnsupportedStorageVersion(version)),
        }
    }

    async fn apply_initial_migration(&self) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(EVENT_TABLE).execute(&mut *transaction).await?;
        sqlx::query(EVENT_TASK_INDEX)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(EVENT_TYPE_INDEX)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO schema_migrations (version, name, applied_at_ms) VALUES (?, ?, ?)",
        )
        .bind(STORAGE_SCHEMA_VERSION)
        .bind("immutable_event_log")
        .bind(now_unix_ms())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    #[cfg(test)]
    async fn rollback_initial_migration(&self) -> Result<(), StorageError> {
        if self.schema_version().await? != STORAGE_SCHEMA_VERSION {
            return Err(StorageError::UnsupportedStorageVersion(
                self.schema_version().await?,
            ));
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DROP INDEX event_log_type_time")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DROP INDEX event_log_task_sequence")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DROP TABLE event_log")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM schema_migrations WHERE version = ?")
            .bind(STORAGE_SCHEMA_VERSION)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn next_sequence(
    transaction: &mut Transaction<'_, Sqlite>,
    stream_id: &str,
) -> Result<u64, StorageError> {
    let tail = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(sequence) FROM event_log WHERE stream_id = ?",
    )
    .bind(stream_id)
    .fetch_one(&mut **transaction)
    .await?
    .unwrap_or(0);
    u64::try_from(tail + 1).map_err(|_| StorageError::CorruptRow("sequence"))
}

fn validate_batch(events: &[EventEnvelope]) -> Result<(), StorageError> {
    let stream_id = &events[0].stream_id;
    let mut expected = events[0].sequence;
    for event in events {
        event.validate()?;
        if &event.stream_id != stream_id {
            return Err(StorageError::MixedStreams);
        }
        if event.sequence != expected {
            return Err(StorageError::UnexpectedSequence {
                expected,
                found: event.sequence,
            });
        }
        reject_sensitive_payload(&event.payload)?;
        expected = expected
            .checked_add(1)
            .ok_or(StorageError::SequenceOutOfRange(event.sequence))?;
    }
    Ok(())
}

fn decode_event(row: SqliteRow) -> Result<EventEnvelope, StorageError> {
    let stored_stream: String = row.try_get("stream_id")?;
    let stored_sequence: i64 = row.try_get("sequence")?;
    let occurred_at_ms: i64 = row.try_get("occurred_at_ms")?;
    let schema_version: i64 = row.try_get("schema_version")?;
    let payload_json: String = row.try_get("payload_json")?;

    Ok(EventEnvelope {
        schema_version: u16::try_from(schema_version)
            .map_err(|_| StorageError::CorruptRow("schema_version"))?,
        event_id: EventId::new(row.try_get::<String, _>("event_id")?)?,
        stream_id: StreamId::new(stored_stream)?,
        task_id: row
            .try_get::<Option<String>, _>("task_id")?
            .map(TaskId::new)
            .transpose()?,
        sequence: u64::try_from(stored_sequence)
            .map_err(|_| StorageError::CorruptRow("sequence"))?,
        occurred_at_ms: u64::try_from(occurred_at_ms)
            .map_err(|_| StorageError::CorruptRow("occurred_at_ms"))?,
        causation_id: row.try_get("causation_id")?,
        correlation_id: row.try_get("correlation_id")?,
        payload: serde_json::from_str::<ApplicationEvent>(&payload_json)?,
    })
}

fn reject_sensitive_payload(payload: &ApplicationEvent) -> Result<(), StorageError> {
    let value = serde_json::to_value(payload)?;
    let mut path = Vec::new();
    scan_value(&value, &mut path)
}

fn scan_value(value: &Value, path: &mut Vec<String>) -> Result<(), StorageError> {
    match value {
        Value::Object(values) => {
            for (key, child) in values {
                path.push(key.clone());
                if is_sensitive_key(key) {
                    return Err(StorageError::SensitiveData(path.join(".")));
                }
                scan_value(child, path)?;
                path.pop();
            }
        }
        Value::Array(values) => {
            for child in values {
                scan_value(child, path)?;
            }
        }
        Value::String(text) if SensitiveDataRedactor::default().contains_sensitive(text) => {
            return Err(StorageError::SensitiveData(path.join(".")));
        }
        _ => {}
    }
    Ok(())
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "apikey"
            | "authorization"
            | "cookie"
            | "customheaders"
            | "environment"
            | "password"
            | "refreshtoken"
            | "secret"
    )
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("a transaction may append events from only one stream")]
    MixedStreams,
    #[error("event sequence is {found}, expected {expected}")]
    UnexpectedSequence { expected: u64, found: u64 },
    #[error("event sequence {0} cannot be represented by SQLite")]
    SequenceOutOfRange(u64),
    #[error("event timestamp {0} cannot be represented by SQLite")]
    TimestampOutOfRange(u64),
    #[error("event payload contains forbidden sensitive data at {0}")]
    SensitiveData(String),
    #[error("unsupported storage schema version {0}")]
    UnsupportedStorageVersion(i64),
    #[error("stored event contains an invalid {0}")]
    CorruptRow(&'static str),
    #[error("the event query is invalid")]
    InvalidQuery,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::{
        ApplicationEvent, EventEnvelope, EventId, StreamId, TaskId, TaskProjection, TaskStatus,
        APPLICATION_CONTRACT_VERSION,
    };

    fn event(sequence: u64, payload: ApplicationEvent) -> EventEnvelope {
        EventEnvelope {
            schema_version: APPLICATION_CONTRACT_VERSION,
            event_id: EventId::new(format!("event-{sequence}")).unwrap(),
            stream_id: StreamId::new("task:test").unwrap(),
            task_id: Some(TaskId::new("test").unwrap()),
            sequence,
            occurred_at_ms: 1_753_731_600_000 + sequence,
            causation_id: Some("command-1".to_owned()),
            correlation_id: Some("turn-1".to_owned()),
            payload,
        }
    }

    fn fixture() -> Vec<EventEnvelope> {
        vec![
            event(
                1,
                ApplicationEvent::TaskCreated {
                    title: "Persist events".to_owned(),
                },
            ),
            event(
                2,
                ApplicationEvent::TaskStatusChanged {
                    status: TaskStatus::Running,
                },
            ),
            event(
                3,
                ApplicationEvent::TurnReceipt {
                    turn_id: kiln_core::TurnId::new("turn-1").unwrap(),
                    outcome: kiln_core::ReceiptOutcome::Completed,
                    summary: "Storage verified.".to_owned(),
                },
            ),
        ]
    }

    #[tokio::test]
    async fn appends_and_replays_an_ordered_stream() {
        let store = SqliteEventStore::in_memory().await.unwrap();
        let expected = fixture();

        store.append(&expected).await.unwrap();
        let replayed = store
            .load_stream(&StreamId::new("task:test").unwrap())
            .await
            .unwrap();

        assert_eq!(replayed, expected);
        assert_eq!(store.event_count().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn rejected_batches_leave_no_partial_events() {
        let store = SqliteEventStore::in_memory().await.unwrap();
        let mut events = fixture();
        events[2].sequence = 4;

        let error = store.append(&events).await.unwrap_err();
        assert!(matches!(
            error,
            StorageError::UnexpectedSequence {
                expected: 3,
                found: 4
            }
        ));
        assert_eq!(store.event_count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn rejects_secret_markers_before_persistence() {
        let store = SqliteEventStore::in_memory().await.unwrap();
        let event = event(
            1,
            ApplicationEvent::MessageAdded {
                message_id: "message-1".to_owned(),
                role: kiln_core::ChatRole::User,
                content: "Authorization: Bearer sk-proj-do-not-store".to_owned(),
            },
        );

        assert!(matches!(
            store.append(&[event]).await,
            Err(StorageError::SensitiveData(_))
        ));
        assert_eq!(store.event_count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn recent_projects_are_derived_from_latest_immutable_events() {
        let store = SqliteEventStore::in_memory().await.unwrap();
        let project_event =
            |stream: &str, event_id: &str, sequence: u64, occurred_at_ms: u64, branch: &str| {
                EventEnvelope {
                    schema_version: APPLICATION_CONTRACT_VERSION,
                    event_id: EventId::new(event_id).unwrap(),
                    stream_id: StreamId::new(stream).unwrap(),
                    task_id: None,
                    sequence,
                    occurred_at_ms,
                    causation_id: Some("command:open-project".to_owned()),
                    correlation_id: None,
                    payload: ApplicationEvent::ProjectOpened {
                        project_id: stream.replace(':', "-"),
                        root: format!("/work/{}", stream.replace(':', "-")),
                        display_name: stream.replace(':', "-"),
                        branch: Some(branch.to_owned()),
                        head: None,
                        status: kiln_core::RepositoryStatus::default(),
                        defaults: kiln_core::ProjectDefaults::default(),
                    },
                }
            };
        store
            .append(&[
                project_event("project:one", "project-one-1", 1, 10, "old"),
                EventEnvelope {
                    schema_version: APPLICATION_CONTRACT_VERSION,
                    event_id: EventId::new("workspace-one-2").unwrap(),
                    stream_id: StreamId::new("project:one").unwrap(),
                    task_id: None,
                    sequence: 2,
                    occurred_at_ms: 11,
                    causation_id: Some("command:open-project".to_owned()),
                    correlation_id: None,
                    payload: ApplicationEvent::WorkspaceReady {
                        workspace_id: "workspace:one".to_owned(),
                        project_id: "project-one".to_owned(),
                        path: "/work/project-one".to_owned(),
                        isolated: false,
                    },
                },
                project_event("project:one", "project-one-3", 3, 30, "main"),
            ])
            .await
            .unwrap();
        store
            .append(&[project_event(
                "project:two",
                "project-two-1",
                1,
                20,
                "develop",
            )])
            .await
            .unwrap();

        let latest = store
            .load_latest_events_by_type("project_opened", 12)
            .await
            .unwrap();
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].stream_id.as_str(), "project:one");
        assert_eq!(latest[0].sequence, 3);
        assert_eq!(latest[1].stream_id.as_str(), "project:two");
        let serialized = serde_json::to_string(&latest).unwrap();
        assert!(!serialized.to_ascii_lowercase().contains("credential"));
        assert!(!serialized.to_ascii_lowercase().contains("apikey"));
    }

    #[tokio::test]
    async fn initial_migration_rolls_back_and_reapplies() {
        let store = SqliteEventStore::in_memory().await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 1);

        store.rollback_initial_migration().await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 0);

        store.migrate_to_latest().await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 1);
        store.append(&fixture()).await.unwrap();
    }

    #[tokio::test]
    async fn repeated_replay_is_byte_stable() {
        let store = SqliteEventStore::in_memory().await.unwrap();
        store.append(&fixture()).await.unwrap();
        let stream = StreamId::new("task:test").unwrap();

        let first = serde_json::to_vec(&store.load_stream(&stream).await.unwrap()).unwrap();
        let second = serde_json::to_vec(&store.load_stream(&stream).await.unwrap()).unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn sqlite_rebuild_matches_the_versioned_projection_snapshot() {
        let store = SqliteEventStore::in_memory().await.unwrap();
        store.append(&fixture()).await.unwrap();
        let events = store
            .load_stream(&StreamId::new("task:test").unwrap())
            .await
            .unwrap();

        let projection = TaskProjection::rebuild(&events).unwrap();
        let actual = serde_json::to_value(&projection).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/task-projection-v1.json"))
                .unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn file_database_reopens_with_the_same_projection() {
        let unique = format!("kiln-storage-{}-{}.db", std::process::id(), now_unix_ms());
        let path = std::env::temp_dir().join(unique);

        let expected = {
            let store = SqliteEventStore::connect_path(&path).await.unwrap();
            store.append(&fixture()).await.unwrap();
            let events = store
                .load_stream(&StreamId::new("task:test").unwrap())
                .await
                .unwrap();
            let projection = TaskProjection::rebuild(&events).unwrap();
            store.pool.close().await;
            projection
        };

        let reopened = SqliteEventStore::connect_path(&path).await.unwrap();
        let events = reopened
            .load_stream(&StreamId::new("task:test").unwrap())
            .await
            .unwrap();
        let actual = TaskProjection::rebuild(&events).unwrap();
        reopened.pool.close().await;
        assert_eq!(actual, expected);

        for suffix in ["", "-shm", "-wal"] {
            let candidate = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
            if candidate.starts_with(std::env::temp_dir()) {
                let _ = std::fs::remove_file(candidate);
            }
        }
    }
}
