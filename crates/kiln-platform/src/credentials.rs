use std::sync::{Arc, Mutex};

use kiln_core::{
    CredentialBackendKind, CredentialBindingState, CredentialProfileRef, ProviderCredentialProfile,
    ProviderKind, ProviderOrigin, SecretString,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SERVICE_NAME: &str = "dev.kiln.credentials";
const PROFILE_PREFIX: &str = "profile:";
const ALIAS_PREFIX: &str = "provider:";
const ALIAS_ENVELOPE_VERSION: u8 = 2;
const MAX_ALIAS_ENVELOPE_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CredentialStoreError {
    #[error("the operating-system credential store is unavailable")]
    Unavailable,
    #[error("the requested credential profile was not found")]
    NotFound,
    #[error("the credential profile does not belong to the selected provider")]
    ProviderMismatch,
    #[error("the credential profile is bound to a different provider destination")]
    DestinationMismatch,
    #[error("the credential profile must be rebound to a provider destination")]
    RebindRequired,
    #[error("the credential value cannot be blank")]
    BlankSecret,
    #[error("a secure credential reference could not be generated")]
    ReferenceGeneration,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AliasEnvelope<'a> {
    v: u8,
    credential_ref: &'a CredentialProfileRef,
    origin: &'a ProviderOrigin,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AliasEnvelopeWire {
    v: u8,
    credential_ref: CredentialProfileRef,
    origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StoredAlias {
    Bound {
        credential_ref: CredentialProfileRef,
        origin: ProviderOrigin,
    },
    Legacy(CredentialProfileRef),
}

impl StoredAlias {
    fn credential_ref(&self) -> &CredentialProfileRef {
        match self {
            Self::Bound { credential_ref, .. } | Self::Legacy(credential_ref) => credential_ref,
        }
    }
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
            let Some(alias) = backend.get(&alias_account(provider))? else {
                continue;
            };
            let alias = parse_alias(alias.expose_secret())?;
            let (origin, binding_state) = match &alias {
                StoredAlias::Bound { origin, .. } => {
                    validate_fixed_origin(provider, origin)?;
                    (Some(origin.clone()), CredentialBindingState::Bound)
                }
                StoredAlias::Legacy(_) => match provider.fixed_official_origin() {
                    Some(origin) => (Some(origin), CredentialBindingState::Bound),
                    None => (None, CredentialBindingState::RebindRequired),
                },
            };
            profiles.push(ProviderCredentialProfile {
                provider,
                credential_ref: alias.credential_ref().clone(),
                backend: backend.kind(),
                origin,
                binding_state,
            });
        }

        Ok(profiles)
    }

    pub fn save(
        &self,
        provider: ProviderKind,
        origin: &ProviderOrigin,
        secret: &SecretString,
    ) -> Result<ProviderCredentialProfile, CredentialStoreError> {
        if secret.is_blank() {
            return Err(CredentialStoreError::BlankSecret);
        }
        validate_fixed_origin(provider, origin)?;

        let credential_ref = generate_reference()?;
        let profile_account_name = profile_account(&credential_ref);
        let alias_account_name = alias_account(provider);
        let alias_secret = encode_alias(&credential_ref, origin)?;
        let backend = self
            .backend
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?;
        let prior_alias_secret = backend.get(&alias_account_name)?;
        let prior_alias = prior_alias_secret
            .as_ref()
            .map(|alias| parse_alias(alias.expose_secret()))
            .transpose()?;

        backend.set(&profile_account_name, secret)?;
        if backend.set(&alias_account_name, &alias_secret).is_err() {
            let _ = backend.delete(&profile_account_name);
            return Err(CredentialStoreError::Unavailable);
        }

        if let Some(prior_alias) = prior_alias {
            let prior_profile = prior_alias.credential_ref();
            let cleanup_failed = prior_profile != &credential_ref
                && backend.delete(&profile_account(prior_profile)).is_err();
            if cleanup_failed {
                if let Some(prior_alias_secret) = prior_alias_secret.as_ref() {
                    let _ = backend.set(&alias_account_name, prior_alias_secret);
                }
                let _ = backend.delete(&profile_account_name);
                return Err(CredentialStoreError::Unavailable);
            }
        }

        Ok(ProviderCredentialProfile {
            provider,
            credential_ref,
            backend: backend.kind(),
            origin: Some(origin.clone()),
            binding_state: CredentialBindingState::Bound,
        })
    }

    pub fn resolve(
        &self,
        provider: ProviderKind,
        origin: &ProviderOrigin,
        credential_ref: &CredentialProfileRef,
    ) -> Result<SecretString, CredentialStoreError> {
        validate_fixed_origin(provider, origin)?;
        let backend = self
            .backend
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?;
        let alias = backend
            .get(&alias_account(provider))?
            .ok_or(CredentialStoreError::NotFound)?;
        let alias = parse_alias(alias.expose_secret())?;
        if alias.credential_ref() != credential_ref {
            return Err(CredentialStoreError::ProviderMismatch);
        }
        match alias {
            StoredAlias::Bound {
                origin: bound_origin,
                ..
            } if bound_origin != *origin => {
                return Err(CredentialStoreError::DestinationMismatch);
            }
            StoredAlias::Bound { .. } => {}
            StoredAlias::Legacy(_) if provider.fixed_official_origin().is_none() => {
                return Err(CredentialStoreError::RebindRequired);
            }
            StoredAlias::Legacy(_) => {}
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
        let alias = backend
            .get(&alias_account(provider))?
            .ok_or(CredentialStoreError::NotFound)?;
        let alias = parse_alias(alias.expose_secret())?;
        if alias.credential_ref() != credential_ref {
            return Err(CredentialStoreError::ProviderMismatch);
        }

        backend.delete(&profile_account(credential_ref))?;
        backend.delete(&alias_account(provider))
    }
}

fn validate_fixed_origin(
    provider: ProviderKind,
    origin: &ProviderOrigin,
) -> Result<(), CredentialStoreError> {
    if provider
        .fixed_official_origin()
        .is_some_and(|official| official != *origin)
    {
        return Err(CredentialStoreError::DestinationMismatch);
    }
    Ok(())
}

fn encode_alias(
    credential_ref: &CredentialProfileRef,
    origin: &ProviderOrigin,
) -> Result<SecretString, CredentialStoreError> {
    let encoded = serde_json::to_string(&AliasEnvelope {
        v: ALIAS_ENVELOPE_VERSION,
        credential_ref,
        origin,
    })
    .map_err(|_| CredentialStoreError::Unavailable)?;
    if encoded.len() > MAX_ALIAS_ENVELOPE_BYTES {
        return Err(CredentialStoreError::Unavailable);
    }
    Ok(SecretString::new(encoded))
}

fn parse_alias(value: &str) -> Result<StoredAlias, CredentialStoreError> {
    if value.len() > MAX_ALIAS_ENVELOPE_BYTES {
        return Err(CredentialStoreError::Unavailable);
    }
    if let Ok(credential_ref) = CredentialProfileRef::new(value) {
        return Ok(StoredAlias::Legacy(credential_ref));
    }

    let envelope: AliasEnvelopeWire =
        serde_json::from_str(value).map_err(|_| CredentialStoreError::Unavailable)?;
    if envelope.v != ALIAS_ENVELOPE_VERSION {
        return Err(CredentialStoreError::Unavailable);
    }
    let origin = ProviderOrigin::from_base_url(&envelope.origin)
        .map_err(|_| CredentialStoreError::Unavailable)?;
    if origin.as_str() != envelope.origin {
        return Err(CredentialStoreError::Unavailable);
    }
    Ok(StoredAlias::Bound {
        credential_ref: envelope.credential_ref,
        origin,
    })
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

    fn origin(base_url: &str) -> ProviderOrigin {
        ProviderOrigin::from_base_url(base_url).unwrap()
    }

    #[test]
    fn save_lists_resolves_and_deletes_an_opaque_profile() {
        let (store, _) = store();
        let destination = ProviderKind::OpenAi.fixed_official_origin().unwrap();
        let saved = store
            .save(
                ProviderKind::OpenAi,
                &destination,
                &SecretString::new("sk-real-secret"),
            )
            .unwrap();

        assert_ne!(saved.credential_ref.as_str(), "sk-real-secret");
        assert_eq!(saved.origin.as_ref(), Some(&destination));
        assert_eq!(saved.binding_state, CredentialBindingState::Bound);
        assert_eq!(store.list_profiles().unwrap(), vec![saved.clone()]);
        assert_eq!(
            store
                .resolve(ProviderKind::OpenAi, &destination, &saved.credential_ref)
                .unwrap()
                .expose_secret(),
            "sk-real-secret"
        );

        store
            .delete(ProviderKind::OpenAi, &saved.credential_ref)
            .unwrap();
        assert!(store.list_profiles().unwrap().is_empty());
        assert_eq!(
            store.resolve(ProviderKind::OpenAi, &destination, &saved.credential_ref),
            Err(CredentialStoreError::NotFound)
        );
    }

    #[test]
    fn provider_binding_prevents_cross_provider_secret_resolution() {
        let (store, _) = store();
        let openai_origin = ProviderKind::OpenAi.fixed_official_origin().unwrap();
        let saved = store
            .save(
                ProviderKind::OpenAi,
                &openai_origin,
                &SecretString::new("sk-real-secret"),
            )
            .unwrap();

        assert_eq!(
            store.resolve(
                ProviderKind::Local,
                &origin("https://gateway.example/v1"),
                &saved.credential_ref
            ),
            Err(CredentialStoreError::NotFound)
        );
    }

    #[test]
    fn compatible_credentials_are_bound_to_the_normalized_origin_not_the_path() {
        let (store, values) = store();
        let destination = origin("https://Gateway.Example:443/v1");
        let saved = store
            .save(
                ProviderKind::Local,
                &destination,
                &SecretString::new("gateway-secret"),
            )
            .unwrap();

        assert_eq!(
            store
                .resolve(
                    ProviderKind::Local,
                    &origin("https://gateway.example/another/api"),
                    &saved.credential_ref,
                )
                .unwrap()
                .expose_secret(),
            "gateway-secret"
        );
        assert_eq!(
            store.resolve(
                ProviderKind::Local,
                &origin("https://other.example/v1"),
                &saved.credential_ref,
            ),
            Err(CredentialStoreError::DestinationMismatch)
        );
        let values = values.lock().unwrap();
        assert!(values.contains_key(&profile_account(&saved.credential_ref)));
        assert!(values.contains_key(&alias_account(ProviderKind::Local)));
    }

    #[test]
    fn first_party_profiles_reject_non_official_origins() {
        let (store, _) = store();
        assert_eq!(
            store.save(
                ProviderKind::OpenAi,
                &origin("https://gateway.example/v1"),
                &SecretString::new("sk-real-secret"),
            ),
            Err(CredentialStoreError::DestinationMismatch)
        );
        assert!(store.list_profiles().unwrap().is_empty());
    }

    #[test]
    fn replacing_a_provider_profile_removes_the_old_secret() {
        let (store, values) = store();
        let destination = ProviderKind::Anthropic.fixed_official_origin().unwrap();
        let first = store
            .save(
                ProviderKind::Anthropic,
                &destination,
                &SecretString::new("first-secret"),
            )
            .unwrap();
        let second = store
            .save(
                ProviderKind::Anthropic,
                &destination,
                &SecretString::new("second-secret"),
            )
            .unwrap();

        assert_eq!(
            store.resolve(ProviderKind::Anthropic, &destination, &first.credential_ref),
            Err(CredentialStoreError::ProviderMismatch)
        );
        assert_eq!(
            store
                .resolve(
                    ProviderKind::Anthropic,
                    &destination,
                    &second.credential_ref,
                )
                .unwrap()
                .expose_secret(),
            "second-secret"
        );
        let values = values.lock().unwrap();
        assert!(!values.contains_key(&profile_account(&first.credential_ref)));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn legacy_cloud_aliases_are_bound_only_to_the_official_origin() {
        let (store, values) = store();
        let credential_ref =
            CredentialProfileRef::new("cred_0123456789abcdef0123456789abcdef").unwrap();
        {
            let mut values = values.lock().unwrap();
            values.insert(
                alias_account(ProviderKind::OpenAi),
                SecretString::new(credential_ref.as_str()),
            );
            values.insert(
                profile_account(&credential_ref),
                SecretString::new("legacy-openai-secret"),
            );
        }

        let profiles = store.list_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].binding_state, CredentialBindingState::Bound);
        assert_eq!(
            profiles[0].origin,
            ProviderKind::OpenAi.fixed_official_origin()
        );
        assert_eq!(
            store
                .resolve(
                    ProviderKind::OpenAi,
                    &ProviderKind::OpenAi.fixed_official_origin().unwrap(),
                    &credential_ref,
                )
                .unwrap()
                .expose_secret(),
            "legacy-openai-secret"
        );
        assert_eq!(
            store.resolve(
                ProviderKind::OpenAi,
                &origin("https://gateway.example/v1"),
                &credential_ref,
            ),
            Err(CredentialStoreError::DestinationMismatch)
        );
    }

    #[test]
    fn legacy_compatible_aliases_require_rebinding_but_remain_deletable() {
        let (store, values) = store();
        let credential_ref =
            CredentialProfileRef::new("cred_fedcba9876543210fedcba9876543210").unwrap();
        {
            let mut values = values.lock().unwrap();
            values.insert(
                alias_account(ProviderKind::Local),
                SecretString::new(credential_ref.as_str()),
            );
            values.insert(
                profile_account(&credential_ref),
                SecretString::new("legacy-local-secret"),
            );
        }

        let profiles = store.list_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].origin, None);
        assert_eq!(
            profiles[0].binding_state,
            CredentialBindingState::RebindRequired
        );
        assert_eq!(
            store.resolve(
                ProviderKind::Local,
                &origin("http://127.0.0.1:11434/v1"),
                &credential_ref,
            ),
            Err(CredentialStoreError::RebindRequired)
        );
        {
            let values = values.lock().unwrap();
            assert!(values.contains_key(&profile_account(&credential_ref)));
            assert!(values.contains_key(&alias_account(ProviderKind::Local)));
        }

        store.delete(ProviderKind::Local, &credential_ref).unwrap();
        assert!(values.lock().unwrap().is_empty());
    }

    #[test]
    fn malformed_or_noncanonical_alias_envelopes_fail_closed() {
        let (store, values) = store();
        let credential_ref =
            CredentialProfileRef::new("cred_0123456789abcdef0123456789abcdef").unwrap();
        {
            let mut values = values.lock().unwrap();
            values.insert(
                alias_account(ProviderKind::Local),
                SecretString::new(format!(
                    r#"{{"v":2,"credentialRef":"{}","origin":"https://gateway.example/v1"}}"#,
                    credential_ref.as_str()
                )),
            );
            values.insert(
                profile_account(&credential_ref),
                SecretString::new("must-remain"),
            );
        }

        assert_eq!(
            store.list_profiles(),
            Err(CredentialStoreError::Unavailable)
        );
        assert_eq!(
            store.resolve(
                ProviderKind::Local,
                &origin("https://gateway.example"),
                &credential_ref,
            ),
            Err(CredentialStoreError::Unavailable)
        );
        let values = values.lock().unwrap();
        assert_eq!(
            values
                .get(&profile_account(&credential_ref))
                .unwrap()
                .expose_secret(),
            "must-remain"
        );
    }

    #[test]
    fn oversized_alias_envelopes_fail_closed() {
        let (store, values) = store();
        values.lock().unwrap().insert(
            alias_account(ProviderKind::Local),
            SecretString::new("x".repeat(MAX_ALIAS_ENVELOPE_BYTES + 1)),
        );
        assert_eq!(
            store.list_profiles(),
            Err(CredentialStoreError::Unavailable)
        );
    }
}
