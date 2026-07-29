use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PermissionResource {
    Tool {
        name: String,
    },
    Command {
        executable: String,
    },
    Path {
        operation: PathOperation,
        path: String,
    },
    NetworkHost {
        host: String,
        port: Option<u16>,
    },
    Extension {
        extension_id: String,
        capability: String,
    },
}

impl PermissionResource {
    fn validate(&self) -> Result<(), PolicyError> {
        match self {
            Self::Tool { name } => validate_text("resource.tool.name", name),
            Self::Command { executable } => {
                validate_text("resource.command.executable", executable)
            }
            Self::Path { path, .. } => validate_text("resource.path.path", path),
            Self::NetworkHost { host, .. } => validate_text("resource.networkHost.host", host),
            Self::Extension {
                extension_id,
                capability,
            } => {
                validate_text("resource.extension.extensionId", extension_id)?;
                validate_text("resource.extension.capability", capability)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathOperation {
    Read,
    Search,
    Write,
    Execute,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ResourceMatcher {
    Any,
    Tool {
        name: String,
    },
    Command {
        executable: String,
    },
    PathPrefix {
        path: String,
        operations: Vec<PathOperation>,
    },
    NetworkHost {
        host: String,
        port: Option<u16>,
    },
    Extension {
        extension_id: String,
        capability: Option<String>,
    },
}

impl ResourceMatcher {
    fn validate(&self) -> Result<(), PolicyError> {
        match self {
            Self::Any => Ok(()),
            Self::Tool { name } => validate_text("matcher.tool.name", name),
            Self::Command { executable } => validate_text("matcher.command.executable", executable),
            Self::PathPrefix { path, operations } => {
                validate_text("matcher.pathPrefix.path", path)?;
                if operations.is_empty() {
                    return Err(PolicyError::InvalidField {
                        field: "matcher.pathPrefix.operations",
                        message: "at least one path operation is required".to_owned(),
                    });
                }
                Ok(())
            }
            Self::NetworkHost { host, .. } => validate_text("matcher.networkHost.host", host),
            Self::Extension {
                extension_id,
                capability,
            } => {
                validate_text("matcher.extension.extensionId", extension_id)?;
                if let Some(capability) = capability {
                    validate_text("matcher.extension.capability", capability)?;
                }
                Ok(())
            }
        }
    }

    fn matches(&self, resource: &PermissionResource) -> bool {
        match (self, resource) {
            (Self::Any, _) => true,
            (Self::Tool { name }, PermissionResource::Tool { name: actual }) => name == actual,
            (Self::Command { executable }, PermissionResource::Command { executable: actual }) => {
                executable == actual
            }
            (
                Self::PathPrefix { path, operations },
                PermissionResource::Path {
                    operation,
                    path: actual,
                },
            ) => operations.contains(operation) && path_is_within(actual, path),
            (
                Self::NetworkHost { host, port },
                PermissionResource::NetworkHost {
                    host: actual_host,
                    port: actual_port,
                },
            ) => normalize_host(host) == normalize_host(actual_host) && port == actual_port,
            (
                Self::Extension {
                    extension_id,
                    capability,
                },
                PermissionResource::Extension {
                    extension_id: actual_id,
                    capability: actual_capability,
                },
            ) => {
                extension_id == actual_id
                    && match capability {
                        Some(expected) => expected == actual_capability,
                        None => true,
                    }
            }
            _ => false,
        }
    }

    fn specificity(&self) -> usize {
        match self {
            Self::Any => 0,
            Self::PathPrefix { path, .. } => 10 + normalize_path(path).split('/').count(),
            Self::Extension {
                capability: None, ..
            } => 20,
            Self::Tool { .. }
            | Self::Command { .. }
            | Self::NetworkHost { .. }
            | Self::Extension {
                capability: Some(_),
                ..
            } => 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ActionOrigin {
    Core,
    Extension { extension_id: String },
}

impl ActionOrigin {
    fn validate(&self) -> Result<(), PolicyError> {
        match self {
            Self::Core => Ok(()),
            Self::Extension { extension_id } => validate_text("origin.extensionId", extension_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OriginMatcher {
    Any,
    Core,
    Extension { extension_id: String },
}

impl OriginMatcher {
    fn validate(&self) -> Result<(), PolicyError> {
        match self {
            Self::Any | Self::Core => Ok(()),
            Self::Extension { extension_id } => {
                validate_text("originMatcher.extensionId", extension_id)
            }
        }
    }

    fn matches(&self, origin: &ActionOrigin) -> bool {
        match (self, origin) {
            (Self::Any, _) | (Self::Core, ActionOrigin::Core) => true,
            (
                Self::Extension { extension_id },
                ActionOrigin::Extension {
                    extension_id: actual,
                },
            ) => extension_id == actual,
            _ => false,
        }
    }

    fn specificity(&self) -> u8 {
        match self {
            Self::Any => 0,
            Self::Core | Self::Extension { .. } => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PolicyTarget {
    Global,
    ProviderProfile { provider_profile_id: String },
    Project { project_id: String },
    Task { task_id: String },
}

impl PolicyTarget {
    fn validate(&self) -> Result<(), PolicyError> {
        match self {
            Self::Global => Ok(()),
            Self::ProviderProfile {
                provider_profile_id,
            } => validate_text("target.providerProfileId", provider_profile_id),
            Self::Project { project_id } => validate_text("target.projectId", project_id),
            Self::Task { task_id } => validate_text("target.taskId", task_id),
        }
    }

    fn matches(&self, context: &PolicyContext) -> bool {
        match self {
            Self::Global => true,
            Self::ProviderProfile {
                provider_profile_id,
            } => context.provider_profile_id.as_ref() == Some(provider_profile_id),
            Self::Project { project_id } => context.project_id.as_ref() == Some(project_id),
            Self::Task { task_id } => context.task_id.as_ref() == Some(task_id),
        }
    }

    fn specificity(&self) -> u8 {
        match self {
            Self::Global => 0,
            Self::ProviderProfile { .. } => 1,
            Self::Project { .. } => 2,
            Self::Task { .. } => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEffect {
    Allow,
    Ask,
    Deny,
}

impl PolicyEffect {
    fn precedence(self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::Ask => 1,
            Self::Deny => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyRule {
    pub rule_id: String,
    pub target: PolicyTarget,
    pub origin: OriginMatcher,
    pub resource: ResourceMatcher,
    pub effect: PolicyEffect,
    pub reason: String,
}

impl PolicyRule {
    fn validate(&self) -> Result<(), PolicyError> {
        validate_text("ruleId", &self.rule_id)?;
        validate_text("reason", &self.reason)?;
        self.target.validate()?;
        self.origin.validate()?;
        self.resource.validate()
    }

    fn matches(&self, proposal: &ActionProposal, context: &PolicyContext) -> bool {
        self.target.matches(context)
            && self.origin.matches(&proposal.origin)
            && self.resource.matches(&proposal.resource)
    }

    fn specificity(&self) -> (u8, u8, usize, u8) {
        (
            self.target.specificity(),
            self.origin.specificity(),
            self.resource.specificity(),
            self.effect.precedence(),
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyContext {
    pub task_id: Option<String>,
    pub project_id: Option<String>,
    pub provider_profile_id: Option<String>,
}

impl PolicyContext {
    fn validate(&self) -> Result<(), PolicyError> {
        for (field, value) in [
            ("context.taskId", &self.task_id),
            ("context.projectId", &self.project_id),
            ("context.providerProfileId", &self.provider_profile_id),
        ] {
            if let Some(value) = value {
                validate_text(field, value)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionProposal {
    pub action_id: String,
    pub origin: ActionOrigin,
    pub resource: PermissionResource,
    pub reason: String,
}

impl ActionProposal {
    pub fn new(
        action_id: impl Into<String>,
        origin: ActionOrigin,
        resource: PermissionResource,
        reason: impl Into<String>,
    ) -> Result<Self, PolicyError> {
        let proposal = Self {
            action_id: action_id.into(),
            origin,
            resource,
            reason: reason.into(),
        };
        proposal.validate()?;
        Ok(proposal)
    }

    fn validate(&self) -> Result<(), PolicyError> {
        validate_text("actionId", &self.action_id)?;
        validate_text("reason", &self.reason)?;
        self.origin.validate()?;
        self.resource.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "decision",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PermissionDecision {
    Allow {
        rule_id: Option<String>,
        ephemeral: bool,
    },
    Ask {
        rule_id: Option<String>,
        reason: String,
    },
    Deny {
        rule_id: String,
        reason: String,
    },
}

impl PermissionDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}

#[derive(Debug)]
pub struct PermissionEngine {
    rules: Vec<PolicyRule>,
    once_grants: BTreeSet<String>,
}

impl PermissionEngine {
    pub fn new(rules: Vec<PolicyRule>) -> Result<Self, PolicyError> {
        let mut seen = BTreeMap::new();
        for rule in &rules {
            rule.validate()?;
            if seen.insert(rule.rule_id.as_str(), ()).is_some() {
                return Err(PolicyError::DuplicateRule(rule.rule_id.clone()));
            }
        }
        Ok(Self {
            rules,
            once_grants: BTreeSet::new(),
        })
    }

    pub fn rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    pub fn evaluate(
        &self,
        proposal: &ActionProposal,
        context: &PolicyContext,
    ) -> Result<PermissionDecision, PolicyError> {
        proposal.validate()?;
        context.validate()?;

        let matched = self
            .rules
            .iter()
            .filter(|rule| rule.matches(proposal, context))
            .max_by_key(|rule| rule.specificity());

        if matched.is_some_and(|rule| rule.effect == PolicyEffect::Deny) {
            let rule = matched.expect("matched deny rule");
            return Ok(PermissionDecision::Deny {
                rule_id: rule.rule_id.clone(),
                reason: rule.reason.clone(),
            });
        }

        if self.once_grants.contains(&proposal.action_id) {
            return Ok(PermissionDecision::Allow {
                rule_id: None,
                ephemeral: true,
            });
        }

        Ok(match matched {
            Some(rule) if rule.effect == PolicyEffect::Allow => PermissionDecision::Allow {
                rule_id: Some(rule.rule_id.clone()),
                ephemeral: false,
            },
            Some(rule) => PermissionDecision::Ask {
                rule_id: Some(rule.rule_id.clone()),
                reason: rule.reason.clone(),
            },
            None => PermissionDecision::Ask {
                rule_id: None,
                reason: "No policy covers this action.".to_owned(),
            },
        })
    }

    pub fn approve_once(
        &mut self,
        proposal: &ActionProposal,
        context: &PolicyContext,
    ) -> Result<(), PolicyError> {
        match self.evaluate(proposal, context)? {
            PermissionDecision::Ask { .. } => {
                self.once_grants.insert(proposal.action_id.clone());
                Ok(())
            }
            decision => Err(PolicyError::ApprovalNotApplicable(decision)),
        }
    }

    pub fn execute<T>(
        &mut self,
        proposal: &ActionProposal,
        context: &PolicyContext,
        operation: impl FnOnce() -> T,
    ) -> Result<T, GuardedExecutionError> {
        let decision = self.evaluate(proposal, context)?;
        if !decision.is_allowed() {
            return Err(GuardedExecutionError::NotAuthorized(decision));
        }

        if matches!(
            decision,
            PermissionDecision::Allow {
                ephemeral: true,
                ..
            }
        ) {
            self.once_grants.remove(&proposal.action_id);
        }
        Ok(operation())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PolicyError {
    #[error("{field}: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("duplicate policy rule {0}")]
    DuplicateRule(String),
    #[error("allow-once approval is not applicable to decision {0:?}")]
    ApprovalNotApplicable(PermissionDecision),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GuardedExecutionError {
    #[error(transparent)]
    InvalidPolicy(#[from] PolicyError),
    #[error("action is not authorized: {0:?}")]
    NotAuthorized(PermissionDecision),
}

fn validate_text(field: &'static str, value: &str) -> Result<(), PolicyError> {
    if value.trim().is_empty() {
        Err(PolicyError::InvalidField {
            field,
            message: "value cannot be blank".to_owned(),
        })
    } else {
        Ok(())
    }
}

fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn path_is_within(candidate: &str, root: &str) -> bool {
    let candidate = normalize_path(candidate);
    let root = normalize_path(root);
    candidate == root || candidate.starts_with(&format!("{root}/"))
}

fn normalize_path(path: &str) -> String {
    let normalized = path.trim().replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn rule(rule_id: &str, resource: ResourceMatcher, effect: PolicyEffect) -> PolicyRule {
        PolicyRule {
            rule_id: rule_id.to_owned(),
            target: PolicyTarget::Global,
            origin: OriginMatcher::Any,
            resource,
            effect,
            reason: format!("{rule_id} policy"),
        }
    }

    fn proposal(action_id: &str, resource: PermissionResource) -> ActionProposal {
        ActionProposal::new(action_id, ActionOrigin::Core, resource, "test action").unwrap()
    }

    #[test]
    fn scopes_every_supported_resource_kind() {
        let rules = vec![
            rule(
                "tool",
                ResourceMatcher::Tool {
                    name: "read_file".to_owned(),
                },
                PolicyEffect::Allow,
            ),
            rule(
                "command",
                ResourceMatcher::Command {
                    executable: "cargo".to_owned(),
                },
                PolicyEffect::Allow,
            ),
            rule(
                "path",
                ResourceMatcher::PathPrefix {
                    path: "C:\\Work Space\\café".to_owned(),
                    operations: vec![PathOperation::Read, PathOperation::Search],
                },
                PolicyEffect::Allow,
            ),
            rule(
                "network",
                ResourceMatcher::NetworkHost {
                    host: "api.example.test".to_owned(),
                    port: Some(443),
                },
                PolicyEffect::Allow,
            ),
            rule(
                "extension",
                ResourceMatcher::Extension {
                    extension_id: "mcp.files".to_owned(),
                    capability: Some("search".to_owned()),
                },
                PolicyEffect::Allow,
            ),
        ];
        let engine = PermissionEngine::new(rules).unwrap();
        let context = PolicyContext::default();
        let resources = [
            PermissionResource::Tool {
                name: "read_file".to_owned(),
            },
            PermissionResource::Command {
                executable: "cargo".to_owned(),
            },
            PermissionResource::Path {
                operation: PathOperation::Read,
                path: "C:\\Work Space\\café\\src\\lib.rs".to_owned(),
            },
            PermissionResource::NetworkHost {
                host: "API.EXAMPLE.TEST.".to_owned(),
                port: Some(443),
            },
            PermissionResource::Extension {
                extension_id: "mcp.files".to_owned(),
                capability: "search".to_owned(),
            },
        ];

        for (index, resource) in resources.into_iter().enumerate() {
            assert!(engine
                .evaluate(&proposal(&format!("action-{index}"), resource), &context)
                .unwrap()
                .is_allowed());
        }
    }

    #[test]
    fn allow_once_is_consumed_and_never_enters_serializable_rules() {
        let mut engine = PermissionEngine::new(Vec::new()).unwrap();
        let context = PolicyContext::default();
        let proposal = proposal(
            "write-once",
            PermissionResource::Path {
                operation: PathOperation::Write,
                path: "/workspace/src/lib.rs".to_owned(),
            },
        );

        assert!(matches!(
            engine.evaluate(&proposal, &context).unwrap(),
            PermissionDecision::Ask { .. }
        ));
        engine.approve_once(&proposal, &context).unwrap();
        assert!(matches!(
            engine.evaluate(&proposal, &context).unwrap(),
            PermissionDecision::Allow {
                ephemeral: true,
                ..
            }
        ));

        let serialized_rules = serde_json::to_string(engine.rules()).unwrap();
        assert!(!serialized_rules.contains("write-once"));
        assert_eq!(
            engine.execute(&proposal, &context, || "executed").unwrap(),
            "executed"
        );
        assert!(matches!(
            engine.evaluate(&proposal, &context).unwrap(),
            PermissionDecision::Ask { .. }
        ));
    }

    #[test]
    fn denied_actions_do_not_invoke_the_operation() {
        let mut engine = PermissionEngine::new(vec![rule(
            "deny-shell",
            ResourceMatcher::Command {
                executable: "powershell".to_owned(),
            },
            PolicyEffect::Deny,
        )])
        .unwrap();
        let side_effects = Cell::new(0);
        let proposal = proposal(
            "shell-1",
            PermissionResource::Command {
                executable: "powershell".to_owned(),
            },
        );

        let result = engine.execute(&proposal, &PolicyContext::default(), || {
            side_effects.set(side_effects.get() + 1);
        });

        assert!(matches!(
            result,
            Err(GuardedExecutionError::NotAuthorized(
                PermissionDecision::Deny { .. }
            ))
        ));
        assert_eq!(side_effects.get(), 0);
    }

    #[test]
    fn extension_specific_deny_overrides_a_general_tool_allow() {
        let rules = vec![
            rule(
                "allow-read",
                ResourceMatcher::Tool {
                    name: "read_file".to_owned(),
                },
                PolicyEffect::Allow,
            ),
            PolicyRule {
                rule_id: "deny-untrusted-extension".to_owned(),
                target: PolicyTarget::Global,
                origin: OriginMatcher::Extension {
                    extension_id: "mcp.untrusted".to_owned(),
                },
                resource: ResourceMatcher::Tool {
                    name: "read_file".to_owned(),
                },
                effect: PolicyEffect::Deny,
                reason: "This extension is not trusted for workspace reads.".to_owned(),
            },
        ];
        let mut engine = PermissionEngine::new(rules).unwrap();
        let proposal = ActionProposal::new(
            "extension-read",
            ActionOrigin::Extension {
                extension_id: "mcp.untrusted".to_owned(),
            },
            PermissionResource::Tool {
                name: "read_file".to_owned(),
            },
            "extension requested a file read",
        )
        .unwrap();
        let side_effects = Cell::new(0);

        let result = engine.execute(&proposal, &PolicyContext::default(), || {
            side_effects.set(1);
        });

        assert!(matches!(
            result,
            Err(GuardedExecutionError::NotAuthorized(
                PermissionDecision::Deny { .. }
            ))
        ));
        assert_eq!(side_effects.get(), 0);
    }

    #[test]
    fn path_prefix_matching_respects_component_boundaries() {
        let engine = PermissionEngine::new(vec![rule(
            "workspace-read",
            ResourceMatcher::PathPrefix {
                path: "/work/kiln".to_owned(),
                operations: vec![PathOperation::Read],
            },
            PolicyEffect::Allow,
        )])
        .unwrap();

        let outside = proposal(
            "outside",
            PermissionResource::Path {
                operation: PathOperation::Read,
                path: "/work/kiln-secrets/file".to_owned(),
            },
        );
        assert!(matches!(
            engine
                .evaluate(&outside, &PolicyContext::default())
                .unwrap(),
            PermissionDecision::Ask { .. }
        ));
    }
}
