//! Cross-platform service boundaries used by the Kiln core.
//!
//! Concrete Windows, Linux, and macOS implementations will live behind these
//! traits. Keeping the contracts in a Tauri-free crate lets the same core run
//! in desktop, CLI, tests, and future headless processes.

mod credentials;

use std::{
    future::Future,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken as TokioCancellationToken;

pub use credentials::{CredentialStoreError, OsCredentialStore};

/// Supplies deterministic wall-clock time to domain services.
pub trait Clock: Send + Sync {
    fn now_unix_ms(&self) -> u64;
}

/// Production clock. Tests can provide a fixed implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

/// Shared cancellation primitive for provider HTTP, tool jobs, and process
/// supervision. Clones belong to one cancellation domain.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    inner: TokioCancellationToken,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    pub async fn cancelled(&self) {
        self.inner.cancelled().await;
    }

    /// Runs one provider or tool future until it completes or its cancellation
    /// domain is cancelled. Cancellation wins a simultaneous race and drops the
    /// active future before returning.
    pub async fn run<T>(
        &self,
        operation: impl Future<Output = T>,
    ) -> Result<T, OperationCancelled> {
        tokio::pin!(operation);
        tokio::select! {
            biased;
            _ = self.cancelled() => Err(OperationCancelled),
            result = &mut operation => Ok(result),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationCancelled;

/// Resolves application-owned paths without coupling the core to a UI shell.
pub trait AppPaths: Send + Sync {
    fn data_dir(&self) -> &Path;
    fn cache_dir(&self) -> &Path;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticAppPaths {
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl StaticAppPaths {
    pub fn new(data_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            cache_dir: cache_dir.into(),
        }
    }
}

impl AppPaths for StaticAppPaths {
    fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use super::*;

    #[test]
    fn static_paths_preserve_platform_specific_values() {
        let paths = StaticAppPaths::new(
            PathBuf::from(r"C:\Users\kiln user\AppData\Roaming\Kiln"),
            PathBuf::from("/tmp/kiln cache"),
        );

        assert_eq!(
            paths.data_dir(),
            Path::new(r"C:\Users\kiln user\AppData\Roaming\Kiln")
        );
        assert_eq!(paths.cache_dir(), Path::new("/tmp/kiln cache"));
    }

    #[tokio::test]
    async fn cancellation_drops_an_active_job_future() {
        struct DropSignal(Arc<AtomicBool>);

        impl Drop for DropSignal {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let token = CancellationToken::default();
        let canceller = token.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let job_dropped = dropped.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();

        let job = tokio::spawn(async move {
            token
                .run(async move {
                    let _drop_signal = DropSignal(job_dropped);
                    let _ = started_tx.send(());
                    std::future::pending::<()>().await;
                })
                .await
        });
        started_rx.await.unwrap();
        canceller.cancel();

        assert_eq!(job.await.unwrap(), Err(OperationCancelled));
        assert!(dropped.load(Ordering::SeqCst));
    }
}
