use std::sync::{Arc, Mutex};

use kiln_core::{
    CredentialBackendKind, CredentialProfileRef, ProviderCredentialProfile, ProviderKind,
    SecretString,
};
use thiserror::Error;

const SERVICE_NAME: &str = "dev.kiln.credentials";
const PROFILE_PREFIX: &str = "profile:";
const ALIAS_PREFIX: &str = "provider:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CredentialStoreError {
    #[error("the operating-system credential store is unavailable")]
    Unavailable,
    #[error("the requested credential profile was not found")]
    NotFound,
    #[error("the credential profile does not belong to the selected provider")]
    ProviderMismatch,
    #[error("the credential value cannot be blank")]
    BlankSecret,
    #[error("a secure credential reference could not be generated")]
    ReferenceGeneration,
}

trait CredentialBackend: Send + Sync {
    fn kind(&self) -> CredentialBackendKind;
    fn get(&self, account: &str) -> Result<Option<SecretString>, CredentialStoreError>;
    fn set(&self, account: &str, secret: &SecretString) -> Result<(), CredentialStoreError>;
    fn delete(&self, account: &str) -> Result<(), CredentialStoreError>;
}

#[derive(Clone)]
pub struct OsCredentialStore {
    backend: Arc<Mutex<Box<dyn CredentialBackend>>>,
}

impl std::fmt::Debug for OsCredentialStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OsCredentialStore")
            .finish_non_exhaustive()
    }
}

impl Default for OsCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl OsCredentialStore {
    pub fn new() -> Self {
        Self::with_backend(PlatformCredentialBackend)
    }

    fn with_backend(backend: impl CredentialBackend + 'static) -> Self {
        Self {
            // Credential Manager has ordering caveats and Secret Service access
            // is blocking at this boundary, so every operation is serialized.
            backend: Arc::new(Mutex::new(Box::new(backend))),
        }
    }

    pub fn list_profiles(&self) -> Result<Vec<ProviderCredentialProfile>, CredentialStoreError> {
        let backend = self
            .backend
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?;
        let mut profiles = Vec::new();

        for provider in ProviderKind::ALL {
            let Some(reference) = backend.get(&alias_account(provider))? else {
                continue;
            };
            let credential_ref = CredentialProfileRef::new(reference.expose_secret())
                .map_err(|_| CredentialStoreError::Unavailable)?;
            profiles.push(ProviderCredentialProfile {
                provider,
                credential_ref,
                backend: backend.kind(),
            });
        }

        Ok(profiles)
    }

    pub fn save(
        &self,
        provider: ProviderKind,
        secret: &SecretString,
    ) -> Result<ProviderCredentialProfile, CredentialStoreError> {
        if secret.is_blank() {
            return Err(CredentialStoreError::BlankSecret);
        }

        let credential_ref = generate_reference()?;
        let profile_account_name = profile_account(&credential_ref);
        let alias_account_name = alias_account(provider);
        let reference_secret = SecretString::new(credential_ref.as_str());
        let backend = self
            .backend
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?;
        let prior_reference = backend.get(&alias_account_name)?;

        backend.set(&profile_account_name, secret)?;
        if backend.set(&alias_account_name, &reference_secret).is_err() {
            let _ = backend.delete(&profile_account_name);
            return Err(CredentialStoreError::Unavailable);
        }

        if let Some(prior_reference) = prior_reference {
            let prior_profile = CredentialProfileRef::new(prior_reference.expose_secret());
            let cleanup_failed = match prior_profile {
                Ok(prior_profile) if prior_profile != credential_ref => {
                    backend.delete(&profile_account(&prior_profile)).is_err()
                }
                Ok(_) => false,
                Err(_) => true,
            };
            if cleanup_failed {
                let _ = backend.set(&alias_account_name, &prior_reference);
                let _ = backend.delete(&profile_account_name);
                return Err(CredentialStoreError::Unavailable);
            }
        }

        Ok(ProviderCredentialProfile {
            provider,
            credential_ref,
            backend: backend.kind(),
        })
    }

    pub fn resolve(
        &self,
        provider: ProviderKind,
        credential_ref: &CredentialProfileRef,
    ) -> Result<SecretString, CredentialStoreError> {
        let backend = self
            .backend
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?;
        let bound_reference = backend
            .get(&alias_account(provider))?
            .ok_or(CredentialStoreError::NotFound)?;
        if bound_reference.expose_secret() != credential_ref.as_str() {
            return Err(CredentialStoreError::ProviderMismatch);
        }

        backend
            .get(&profile_account(credential_ref))?
            .ok_or(CredentialStoreError::NotFound)
    }

    pub fn delete(
        &self,
        provider: ProviderKind,
        credential_ref: &CredentialProfileRef,
    ) -> Result<(), CredentialStoreError> {
        let backend = self
            .backend
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?;
        let bound_reference = backend
            .get(&alias_account(provider))?
            .ok_or(CredentialStoreError::NotFound)?;
        if bound_reference.expose_secret() != credential_ref.as_str() {
            return Err(CredentialStoreError::ProviderMismatch);
        }

        backend.delete(&profile_account(credential_ref))?;
        backend.delete(&alias_account(provider))
    }
}

fn generate_reference() -> Result<CredentialProfileRef, CredentialStoreError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| CredentialStoreError::ReferenceGeneration)?;
    CredentialProfileRef::new(format!("cred_{}", hex::encode(bytes)))
        .map_err(|_| CredentialStoreError::ReferenceGeneration)
}

fn alias_account(provider: ProviderKind) -> String {
    format!("{ALIAS_PREFIX}{}", provider.as_str())
}

fn profile_account(credential_ref: &CredentialProfileRef) -> String {
    format!("{PROFILE_PREFIX}{}", credential_ref.as_str())
}

#[derive(Debug, Clone, Copy)]
struct PlatformCredentialBackend;

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl CredentialBackend for PlatformCredentialBackend {
    fn kind(&self) -> CredentialBackendKind {
        #[cfg(target_os = "windows")]
        return CredentialBackendKind::WindowsCredentialManager;
        #[cfg(target_os = "linux")]
        return CredentialBackendKind::LinuxSecretService;
    }

    fn get(&self, account: &str) -> Result<Option<SecretString>, CredentialStoreError> {
        let entry = keyring::Entry::new(SERVICE_NAME, account)
            .map_err(|_| CredentialStoreError::Unavailable)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(SecretString::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CredentialStoreError::Unavailable),
        }
    }

    fn set(&self, account: &str, secret: &SecretString) -> Result<(), CredentialStoreError> {
        keyring::Entry::new(SERVICE_NAME, account)
            .and_then(|entry| entry.set_password(secret.expose_secret()))
            .map_err(|_| CredentialStoreError::Unavailable)
    }

    fn delete(&self, account: &str) -> Result<(), CredentialStoreError> {
        let entry = keyring::Entry::new(SERVICE_NAME, account)
            .map_err(|_| CredentialStoreError::Unavailable)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialStoreError::Unavailable),
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl CredentialBackend for PlatformCredentialBackend {
    fn kind(&self) -> CredentialBackendKind {
        unreachable!("OS credential storage is supported on Windows and Linux")
    }

    fn get(&self, _account: &str) -> Result<Option<SecretString>, CredentialStoreError> {
        Err(CredentialStoreError::Unavailable)
    }

    fn set(&self, _account: &str, _secret: &SecretString) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::Unavailable)
    }

    fn delete(&self, _account: &str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use super::*;

    #[derive(Clone)]
    struct MemoryBackend {
        values: Arc<Mutex<BTreeMap<String, SecretString>>>,
    }

    impl CredentialBackend for MemoryBackend {
        fn kind(&self) -> CredentialBackendKind {
            CredentialBackendKind::WindowsCredentialManager
        }

        fn get(&self, account: &str) -> Result<Option<SecretString>, CredentialStoreError> {
            Ok(self.values.lock().unwrap().get(account).cloned())
        }

        fn set(&self, account: &str, secret: &SecretString) -> Result<(), CredentialStoreError> {
            self.values
                .lock()
                .unwrap()
                .insert(account.to_owned(), secret.clone());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), CredentialStoreError> {
            self.values.lock().unwrap().remove(account);
            Ok(())
        }
    }

    type MemoryValues = Arc<Mutex<BTreeMap<String, SecretString>>>;

    fn store() -> (OsCredentialStore, MemoryValues) {
        let values = Arc::new(Mutex::new(BTreeMap::new()));
        (
            OsCredentialStore::with_backend(MemoryBackend {
                values: values.clone(),
            }),
            values,
        )
    }

    #[test]
    fn save_lists_resolves_and_deletes_an_opaque_profile() {
        let (store, _) = store();
        let saved = store
            .save(ProviderKind::OpenAi, &SecretString::new("sk-real-secret"))
            .unwrap();

        assert_ne!(saved.credential_ref.as_str(), "sk-real-secret");
        assert_eq!(store.list_profiles().unwrap(), vec![saved.clone()]);
        assert_eq!(
            store
                .resolve(ProviderKind::OpenAi, &saved.credential_ref)
                .unwrap()
                .expose_secret(),
            "sk-real-secret"
        );

        store
            .delete(ProviderKind::OpenAi, &saved.credential_ref)
            .unwrap();
        assert!(store.list_profiles().unwrap().is_empty());
        assert_eq!(
            store.resolve(ProviderKind::OpenAi, &saved.credential_ref),
            Err(CredentialStoreError::NotFound)
        );
    }

    #[test]
    fn provider_binding_prevents_cross_provider_secret_resolution() {
        let (store, _) = store();
        let saved = store
            .save(ProviderKind::OpenAi, &SecretString::new("sk-real-secret"))
            .unwrap();

        assert_eq!(
            store.resolve(ProviderKind::Local, &saved.credential_ref),
            Err(CredentialStoreError::NotFound)
        );
    }

    #[test]
    fn replacing_a_provider_profile_removes_the_old_secret() {
        let (store, values) = store();
        let first = store
            .save(ProviderKind::Anthropic, &SecretString::new("first-secret"))
            .unwrap();
        let second = store
            .save(ProviderKind::Anthropic, &SecretString::new("second-secret"))
            .unwrap();

        assert_eq!(
            store.resolve(ProviderKind::Anthropic, &first.credential_ref),
            Err(CredentialStoreError::ProviderMismatch)
        );
        assert_eq!(
            store
                .resolve(ProviderKind::Anthropic, &second.credential_ref)
                .unwrap()
                .expose_secret(),
            "second-secret"
        );
        let values = values.lock().unwrap();
        assert!(!values.contains_key(&profile_account(&first.credential_ref)));
        assert_eq!(values.len(), 2);
    }
}
