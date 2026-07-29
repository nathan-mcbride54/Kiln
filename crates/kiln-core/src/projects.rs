use serde::{Deserialize, Serialize};

use crate::{ContractError, ProviderKind};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_profile: Option<String>,
}

impl ProjectDefaults {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_optional_text("defaults.model", self.model.as_deref())?;
        validate_optional_text(
            "defaults.verificationProfile",
            self.verification_profile.as_deref(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStatus {
    #[serde(default)]
    pub staged: u32,
    #[serde(default)]
    pub modified: u32,
    #[serde(default)]
    pub untracked: u32,
    #[serde(default)]
    pub conflicts: u32,
    #[serde(default)]
    pub ahead: u32,
    #[serde(default)]
    pub behind: u32,
}

impl RepositoryStatus {
    pub fn is_clean(&self) -> bool {
        self.staged == 0 && self.modified == 0 && self.untracked == 0 && self.conflicts == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub project_id: String,
    pub display_name: String,
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default)]
    pub status: RepositoryStatus,
    #[serde(default)]
    pub defaults: ProjectDefaults,
}

impl ProjectSnapshot {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_text("projectId", &self.project_id)?;
        validate_text("displayName", &self.display_name)?;
        validate_text("root", &self.root)?;
        validate_optional_text("branch", self.branch.as_deref())?;
        validate_optional_text("head", self.head.as_deref())?;
        self.defaults.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectRequest {
    pub path: String,
    #[serde(default)]
    pub defaults: ProjectDefaults,
}

impl OpenProjectRequest {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_text("path", &self.path)?;
        self.defaults.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RememberedProject {
    pub project: ProjectSnapshot,
    pub last_opened_at_ms: u64,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ContractError> {
    if value.trim().is_empty() {
        Err(ContractError::InvalidField {
            field,
            message: "value cannot be blank".to_owned(),
        })
    } else {
        Ok(())
    }
}

fn validate_optional_text(field: &'static str, value: Option<&str>) -> Result<(), ContractError> {
    if value.is_some_and(|text| text.trim().is_empty()) {
        Err(ContractError::InvalidField {
            field,
            message: "value cannot be blank when present".to_owned(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_cleanliness_ignores_sync_distance() {
        let status = RepositoryStatus {
            ahead: 2,
            behind: 1,
            ..RepositoryStatus::default()
        };

        assert!(status.is_clean());
    }

    #[test]
    fn project_defaults_have_no_credential_shape() {
        let defaults = ProjectDefaults {
            provider: Some(ProviderKind::OpenAi),
            model: Some("gpt-5".to_owned()),
            verification_profile: Some("quick".to_owned()),
        };
        let json = serde_json::to_value(defaults).unwrap();

        assert!(json.get("apiKey").is_none());
        assert!(json.get("credentials").is_none());
    }
}
