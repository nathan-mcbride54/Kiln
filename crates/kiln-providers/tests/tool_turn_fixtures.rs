use kiln_core::{ProviderProtocol, RepositoryToolRequest};
use kiln_providers::{ProviderTurnEvent, ToolTurnCodec};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolTurnFixture {
    version: u32,
    logical_request: RepositoryToolRequest,
    providers: Vec<ProviderFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderFixture {
    name: String,
    protocol: ProviderProtocol,
    sensitive_handle: String,
    events: Vec<Value>,
}

#[test]
fn recorded_provider_streams_share_one_repository_request_contract() {
    let fixture: ToolTurnFixture =
        serde_json::from_str(include_str!("fixtures/tool-turns-v1.json")).unwrap();
    assert_eq!(fixture.version, 1);
    assert_eq!(fixture.providers.len(), 3);

    for provider in fixture.providers {
        let mut codec = ToolTurnCodec::new(provider.protocol);
        let mut calls = Vec::new();
        for event in provider.events {
            let data = event
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| event.to_string());
            for event in codec
                .push(&data)
                .unwrap_or_else(|error| panic!("{} fixture failed: {error}", provider.name))
            {
                if let ProviderTurnEvent::ToolCall { call } = event {
                    calls.push(call);
                }
            }
        }
        codec
            .finish()
            .unwrap_or_else(|error| panic!("{} fixture did not finish: {error}", provider.name));
        assert_eq!(calls.len(), 1, "{} call count", provider.name);
        assert_eq!(
            calls[0].repository_request().unwrap(),
            fixture.logical_request,
            "{} normalized request",
            provider.name
        );

        let debug = format!("{:?}", calls[0]);
        assert!(!debug.contains(&provider.sensitive_handle));
        assert!(!debug.contains("src/lib.rs"));
    }
}
