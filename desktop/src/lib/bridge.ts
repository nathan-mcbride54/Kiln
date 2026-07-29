import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ApplicationEvent,
  EventEnvelope,
} from "./events.ts";
import type {
  ChatRequest,
  ChatResponse,
  CommandError,
  ConnectionProbe,
  ConnectionTestResponse,
  ProjectDefaults,
  ProjectSnapshot,
  ProviderCapabilities,
  ProviderConfig,
  ProviderCredentialProfile,
  ProviderId,
  RememberedProject,
  RepositoryToolExecution,
  RepositoryToolRequest,
  RepositoryToolResult,
} from "./types";
import {
  applicationEventsFromStream,
  type DesktopStreamEvent,
  type TurnExecutionContext,
} from "./stream-events.ts";

const previewActiveTurns = new Set<string>();
const previewCancelledTurns = new Set<string>();
const previewCredentialProfiles = new Map<ProviderId, ProviderCredentialProfile>();
let previewProject: ProjectSnapshot = {
  projectId: "project-preview-kiln",
  displayName: "kiln",
  root: "D:\\Projects\\kiln",
  branch: "main",
  head: "preview",
  status: {
    staged: 0,
    modified: 1,
    untracked: 0,
    conflicts: 0,
    ahead: 0,
    behind: 0,
  },
  defaults: {
    provider: "openai",
    model: "gpt-5.6-terra",
  },
};

class DesktopCommandError extends Error {
  readonly code?: string;

  constructor(message: string, code?: string) {
    super(message);
    this.name = "DesktopCommandError";
    this.code = code;
  }
}

const previewCapabilities: ProviderCapabilities[] = [
  {
    provider: "openai",
    displayName: "OpenAI",
    protocol: "open_ai_responses",
    defaultBaseUrl: "https://api.openai.com/v1",
    apiKeyRequired: true,
    customBaseUrl: false,
    customHeaders: false,
    modelDiscovery: true,
    streaming: true,
    toolCalling: true,
    systemMessages: true,
    temperature: true,
  },
  {
    provider: "anthropic",
    displayName: "Anthropic",
    protocol: "anthropic_messages",
    defaultBaseUrl: "https://api.anthropic.com/v1",
    apiKeyRequired: true,
    customBaseUrl: false,
    customHeaders: false,
    modelDiscovery: true,
    streaming: true,
    toolCalling: true,
    systemMessages: true,
    temperature: true,
  },
  {
    provider: "local",
    displayName: "Local server",
    protocol: "open_ai_chat_completions",
    defaultBaseUrl: "http://127.0.0.1:11434/v1",
    apiKeyRequired: false,
    customBaseUrl: true,
    customHeaders: true,
    modelDiscovery: true,
    streaming: true,
    toolCalling: true,
    systemMessages: true,
    temperature: true,
  },
];

export function isDesktopRuntime(): boolean {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => globalThis.setTimeout(resolve, ms));
}

async function callOrPreview<T>(
  command: string,
  args: Record<string, unknown>,
  preview: () => Promise<T> | T,
): Promise<T> {
  if (!isDesktopRuntime()) {
    if (typeof window !== "undefined") {
      await wait(520);
    }
    return preview();
  }

  return invokeDesktop(command, args);
}

async function invokeDesktop<T>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const commandError = error as CommandError | string;
    if (typeof commandError === "string") {
      throw new Error(commandError);
    }

    throw new DesktopCommandError(
      commandError.message ??
        `Kiln could not complete “${command.replaceAll("_", " ")}”.`,
      commandError.code,
    );
  }
}

export function listProviderCapabilities(): Promise<ProviderCapabilities[]> {
  return callOrPreview(
    "list_provider_capabilities",
    {},
    () => previewCapabilities,
  );
}

export function listProviderCredentials(): Promise<ProviderCredentialProfile[]> {
  return callOrPreview(
    "list_provider_credentials",
    {},
    () => [...previewCredentialProfiles.values()],
  );
}

export function saveProviderCredential(
  provider: ProviderId,
  secret: string,
  baseUrl: string,
): Promise<ProviderCredentialProfile> {
  return callOrPreview(
    "save_provider_credential",
    { request: { provider, secret, baseUrl } },
    () => {
      const origin = canonicalProviderOrigin(baseUrl);
      const profile: ProviderCredentialProfile = {
        provider,
        credentialRef: `cred_${crypto.randomUUID().replaceAll("-", "").slice(0, 32)}`,
        backend:
          navigator.platform.toLowerCase().includes("win")
            ? "windows_credential_manager"
            : "linux_secret_service",
        origin,
        bindingState: "bound",
      };
      previewCredentialProfiles.set(provider, profile);
      return profile;
    },
  );
}

export function deleteProviderCredential(
  profile: ProviderCredentialProfile,
): Promise<void> {
  return callOrPreview(
    "delete_provider_credential",
    {
      provider: profile.provider,
      credentialRef: profile.credentialRef,
    },
    () => {
      const candidate = previewCredentialProfiles.get(profile.provider);
      if (candidate?.credentialRef === profile.credentialRef) {
        previewCredentialProfiles.delete(profile.provider);
      }
    },
  );
}

export function canonicalProviderOrigin(baseUrl: string): string | undefined {
  try {
    const url = new URL(baseUrl.trim());
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return undefined;
    }
    if (url.username || url.password || url.search || url.hash) {
      return undefined;
    }
    return url.origin;
  } catch {
    return undefined;
  }
}

export function usableProviderCredential(
  provider: ProviderConfig,
): string | undefined {
  if (
    !provider.credentialRef ||
    provider.credentialBindingState !== "bound"
  ) {
    return undefined;
  }

  if (!provider.credentialOrigin) return provider.credentialRef;
  const configuredOrigin = canonicalProviderOrigin(provider.baseUrl);
  const credentialOrigin =
    canonicalProviderOrigin(provider.credentialOrigin) ??
    provider.credentialOrigin.trim();
  return configuredOrigin === credentialOrigin
    ? provider.credentialRef
    : undefined;
}

export function testProviderConnection(
  provider: ProviderConfig,
): Promise<ConnectionTestResponse> {
  return callOrPreview(
    "test_connection",
    {
      request: {
        provider: provider.id,
        credentialRef: usableProviderCredential(provider),
        baseUrl: provider.baseUrl || undefined,
        model: provider.model.trim() || undefined,
      },
    },
    () => previewConnectionReport(provider),
  );
}

function previewConnectionReport(
  provider: ProviderConfig,
): ConnectionTestResponse {
  const contract =
    provider.capabilities ??
    previewCapabilities.find(
      (candidate) => candidate.provider === provider.id,
    );
  const hasAuthentication =
    !contract?.apiKeyRequired || Boolean(usableProviderCredential(provider));
  const model = provider.model.trim() || undefined;
  const models = contract?.modelDiscovery && model ? [model] : [];
  const probes: ConnectionProbe[] = [
    {
      kind: "reachability",
      status: "passed",
      latencyMs: 28,
      message: "Preview endpoint responded.",
    },
    {
      kind: "authentication",
      status: hasAuthentication ? "passed" : "failed",
      message: hasAuthentication
        ? contract?.apiKeyRequired
          ? "The stored credential was accepted in preview."
          : "This profile does not require a credential."
        : "Save a credential for this destination to authenticate.",
    },
    {
      kind: "model_discovery",
      status: contract?.modelDiscovery ? "passed" : "unsupported",
      message: contract?.modelDiscovery
        ? `Preview discovered ${models.length} configured model${models.length === 1 ? "" : "s"}.`
        : "This adapter does not advertise model discovery.",
    },
    previewGenerationProbe(
      "streaming",
      Boolean(contract?.streaming),
      hasAuthentication,
      model,
    ),
    previewGenerationProbe(
      "tool_compatibility",
      Boolean(contract?.toolCalling),
      hasAuthentication,
      model,
    ),
  ];
  const overall = !hasAuthentication
    ? "unavailable"
    : probes.every((probe) => probe.status === "passed")
      ? "ready"
      : "degraded";

  return {
    provider: provider.id,
    origin:
      canonicalProviderOrigin(provider.baseUrl) ?? provider.baseUrl.trim(),
    model,
    overall,
    models,
    probes,
  };
}

function previewGenerationProbe(
  kind: "streaming" | "tool_compatibility",
  supported: boolean,
  authenticated: boolean,
  model: string | undefined,
): ConnectionProbe {
  const feature = kind === "streaming" ? "Streaming" : "Tool calling";
  if (!authenticated) {
    return {
      kind,
      status: "skipped",
      message: `${feature} was skipped because authentication failed.`,
    };
  }
  if (!model) {
    return {
      kind,
      status: "skipped",
      message: `${feature} needs a model selection.`,
    };
  }
  return supported
    ? {
        kind,
        status: "passed",
        latencyMs: kind === "streaming" ? 36 : 42,
        message: `${feature} passed with a small preview request.`,
      }
    : {
        kind,
        status: "unsupported",
        message: `This adapter does not advertise ${feature.toLowerCase()}.`,
      };
}

export async function loadApplicationEvents(
  streamId: string,
): Promise<readonly EventEnvelope[]> {
  if (!isDesktopRuntime()) return [];
  return invokeDesktop<EventEnvelope[]>("load_application_events", {
    streamId,
  });
}

export async function persistApplicationEvents(
  events: readonly EventEnvelope[],
): Promise<void> {
  if (!isDesktopRuntime() || events.length === 0) return;
  await invokeDesktop<void>("append_application_events", { events });
}

export function listRememberedProjects(): Promise<RememberedProject[]> {
  return callOrPreview(
    "list_remembered_projects",
    {},
    () => [
      {
        project: previewProject,
        lastOpenedAtMs: Date.now(),
        available: true,
      },
    ],
  );
}

export function openRepository(
  path: string,
  defaults: ProjectDefaults,
): Promise<ProjectSnapshot> {
  return callOrPreview(
    "open_repository",
    { request: { path, defaults } },
    () => {
      const normalized = path.trim() || previewProject.root;
      const parts = normalized.split(/[\\/]/).filter(Boolean);
      previewProject = {
        ...previewProject,
        projectId: `project-preview-${parts.at(-1) ?? "repository"}`,
        displayName: parts.at(-1) ?? "repository",
        root: normalized,
        defaults,
      };
      return previewProject;
    },
  );
}

export function executeRepositoryTool(
  projectId: string,
  toolCallId: string,
  request: RepositoryToolRequest,
  turnId?: string,
): Promise<RepositoryToolExecution> {
  return callOrPreview(
    "execute_repository_tool",
    { projectId, toolCallId, request, turnId },
    () => previewRepositoryTool(request),
  );
}

export async function executeVisibleRepositoryTool(
  projectId: string,
  toolCallId: string,
  request: RepositoryToolRequest,
  consume: (events: readonly ApplicationEvent[]) => Promise<void>,
  turnId?: string,
): Promise<RepositoryToolResult> {
  const approvalId = `approval:${toolCallId}`;
  const proposed: ApplicationEvent[] = [
    {
      type: "tool_proposed",
      data: {
        toolCallId,
        name: request.tool,
        summary: repositoryToolProposalSummary(request),
      },
    },
    { type: "tool_started", data: { toolCallId } },
  ];
  if (request.tool === "write_file") {
    proposed.unshift({
      type: "approval_requested",
      data: {
        approvalId,
        action: "write_file",
        resource: request.input.path,
        reason: "Apply one version-checked, atomic workspace edit.",
      },
    });
  }
  await consume(proposed);

  let execution: RepositoryToolExecution;
  try {
    execution = await executeRepositoryTool(
      projectId,
      toolCallId,
      request,
      turnId,
    );
  } catch (error) {
    const failed: ApplicationEvent[] = [
      {
        type: "tool_output",
        data: {
          toolCallId,
          stream: "stderr",
          chunk: "Repository tool failed.",
        },
      },
      {
        type: "tool_completed",
        data: { toolCallId, success: false },
      },
    ];
    if (request.tool === "write_file") {
      failed.unshift({
        type: "approval_decided",
        data: {
          approvalId,
          decision:
            error instanceof DesktopCommandError &&
            error.code === "permission_denied"
              ? "denied"
              : "approved",
          scope: "once",
        },
      });
    }
    await consume(failed);
    throw error;
  }

  const completed: ApplicationEvent[] = [
    {
      type: "tool_output",
      data: {
        toolCallId,
        stream: "structured",
        chunk: execution.activitySummary,
      },
    },
    {
      type: "tool_completed",
      data: { toolCallId, success: true },
    },
  ];
  if (execution.result.tool === "write_file") {
    completed.unshift({
      type: "approval_decided",
      data: { approvalId, decision: "approved", scope: "once" },
    });
    completed.push({
      type: "artifact_published",
      data: {
        artifactId: `artifact:${toolCallId}`,
        kind: "diff",
        label: execution.result.result.path,
      },
    });
  }
  await consume(completed);
  return execution.result;
}

function previewRepositoryTool(
  request: RepositoryToolRequest,
): RepositoryToolExecution {
  if (request.tool === "read_file") {
    const result: RepositoryToolResult = {
      tool: "read_file",
      result: {
        path: request.input.path,
        content: "// Desktop preview — launch Kiln for a live workspace read.\n",
        startLine: request.input.startLine ?? 1,
        endLine: request.input.startLine ?? 1,
        truncated: false,
        sha256: "0".repeat(64),
      },
    };
    return {
      result,
      activitySummary: repositoryToolPreviewSummary(result),
    };
  }
  if (request.tool === "search_files") {
    const result: RepositoryToolResult = {
      tool: "search_files",
      result: {
        pattern: request.input.pattern,
        matches: [
          { path: "desktop/src/App.svelte" },
          { path: "crates/kiln-core/src/lib.rs" },
          { path: "README.md" },
        ],
        truncated: false,
      },
    };
    return {
      result,
      activitySummary: repositoryToolPreviewSummary(result),
    };
  }
  if (request.tool === "write_file") {
    const created = request.input.expectedSha256 === undefined;
    const result: RepositoryToolResult = {
      tool: "write_file",
      result: {
        path: request.input.path,
        created,
        bytesWritten: new TextEncoder().encode(request.input.content).length,
        beforeSha256: request.input.expectedSha256,
        afterSha256: "1".repeat(64),
        unifiedDiff:
          `--- ${created ? "/dev/null" : `a/${request.input.path}`}\n` +
          `+++ b/${request.input.path}\n` +
          `@@ -1,0 +1,1 @@\n+${request.input.content}`,
      },
    };
    return {
      result,
      activitySummary: repositoryToolPreviewSummary(result),
    };
  }
  const result: RepositoryToolResult = {
    tool: "search_text",
    result: {
      query: request.input.query,
      matches: [],
      filesSearched: 3,
      truncated: false,
    },
  };
  return {
    result,
    activitySummary: repositoryToolPreviewSummary(result),
  };
}

function repositoryToolProposalSummary(
  request: RepositoryToolRequest,
): string {
  if (request.tool === "read_file") {
    return `Read ${request.input.path} inside the selected workspace`;
  }
  if (request.tool === "search_files") {
    return "Search file paths inside the selected workspace";
  }
  if (request.tool === "write_file") {
    return `Write ${request.input.path} after native approval`;
  }
  return "Search text inside the selected workspace";
}

function repositoryToolPreviewSummary(
  output: RepositoryToolResult,
): string {
  if (output.tool === "read_file") {
    const suffix = output.result.truncated ? " · more available" : "";
    return (
      `Read lines ${output.result.startLine}–${output.result.endLine} ` +
      `from ${output.result.path}${suffix}`
    );
  }
  if (output.tool === "search_files") {
    const count = output.result.matches.length;
    return `Found ${count} workspace file${count === 1 ? "" : "s"}`;
  }
  if (output.tool === "write_file") {
    return `${output.result.created ? "Created" : "Updated"} ${output.result.path} with an atomic workspace edit`;
  }
  const count = output.result.matches.length;
  return (
    `Found ${count} text match${count === 1 ? "" : "es"} across ` +
    `${output.result.filesSearched} file${output.result.filesSearched === 1 ? "" : "s"}`
  );
}

function sendChat(request: ChatRequest): Promise<ChatResponse> {
  return callOrPreview(
    "send_chat_request",
    { request },
    async () => {
      const providerName =
        request.provider === "openai"
          ? "OpenAI"
          : request.provider === "anthropic"
            ? "Anthropic"
            : "your local server";

      return {
        provider: request.provider,
        id: `preview-${Date.now()}`,
        model: request.model,
        content:
          `I mapped the provider boundary and kept the execution path portable. ` +
          `In the desktop runtime this message will come from ${providerName}; ` +
          `the browser uses a safe preview so the workbench remains explorable.\n\n` +
          `Next I’d inspect the target files, propose a compact change set, and pause at any command that needs your approval.`,
        finishReason: "stop",
        usage: {
          inputTokens: 63,
          outputTokens: 74,
          totalTokens: 137,
        },
      };
    },
  );
}

export async function executeTurnStreaming(
  request: ChatRequest,
  context: TurnExecutionContext,
  consume: (events: readonly ApplicationEvent[]) => Promise<void>,
): Promise<void> {
  if (!isDesktopRuntime()) {
    await executePreviewStream(request, context, consume);
    return;
  }

  const onEvent = new Channel<DesktopStreamEvent>();
  let processing = Promise.resolve();
  let settled = false;
  let resolveTerminal!: () => void;
  let rejectTerminal!: (error: unknown) => void;
  const terminal = new Promise<void>((resolve, reject) => {
    resolveTerminal = resolve;
    rejectTerminal = reject;
  });

  onEvent.onmessage = (message) => {
    if (settled) return;
    processing = processing
      .then(async () => {
        if (settled) return;
        const batch = applicationEventsFromStream(message, request, context);
        await consume(batch.events);
        if (batch.terminal) {
          settled = true;
          resolveTerminal();
        }
      })
      .catch((error) => {
        settled = true;
        rejectTerminal(error);
      });
  };

  try {
    await invokeDesktop<void>("start_chat_stream", {
      turnId: context.turnId,
      request,
      onEvent,
    });
  } catch (error) {
    const summary =
      error instanceof Error
        ? error.message
        : "The selected provider could not start this request.";
    await consume(
      applicationEventsFromStream(
        {
          type: "error",
          data: { error: { message: summary } },
        },
        request,
        context,
      ).events,
    );
    settled = true;
    return;
  }
  await terminal;
  await processing;
}

export async function cancelTurn(turnId: string): Promise<boolean> {
  if (!isDesktopRuntime()) {
    if (!previewActiveTurns.has(turnId)) return false;
    previewCancelledTurns.add(turnId);
    return true;
  }
  return invokeDesktop<boolean>("cancel_turn", { turnId });
}

async function executePreviewStream(
  request: ChatRequest,
  context: TurnExecutionContext,
  consume: (events: readonly ApplicationEvent[]) => Promise<void>,
): Promise<void> {
  const content =
    "I mapped the provider boundary and kept the execution path portable. " +
    "The same ordered stream now carries visible text, cancellation, and its completion receipt.";
  const deltas = [
    "I mapped the provider boundary ",
    "and kept the execution path portable. ",
    "The same ordered stream now carries visible text, ",
    "cancellation, and its completion receipt.",
  ];
  previewActiveTurns.add(context.turnId);
  previewCancelledTurns.delete(context.turnId);

  try {
    for (const delta of deltas) {
      await wait(160);
      if (previewCancelledTurns.has(context.turnId)) {
        await consume([
          {
            type: "turn_receipt",
            data: {
              turnId: context.turnId,
              outcome: "cancelled",
              summary: "The preview turn was cancelled.",
            },
          },
        ]);
        return;
      }
      await consume([
        {
          type: "message_delta",
          data: { messageId: context.assistantMessageId, delta },
        },
      ]);
    }
    await consume([
      {
        type: "message_completed",
        data: {
          messageId: context.assistantMessageId,
          model: request.model,
          content,
          finishReason: "stop",
          usage: {
            inputTokens: 63,
            outputTokens: 30,
            totalTokens: 93,
          },
        },
      },
      {
        type: "turn_receipt",
        data: {
          turnId: context.turnId,
          outcome: "completed",
          summary: "Preview provider stream completed.",
        },
      },
    ]);
  } finally {
    previewActiveTurns.delete(context.turnId);
    previewCancelledTurns.delete(context.turnId);
  }
}

/// Converts transport/provider results into the application event vocabulary.
/// Svelte consumes these events and never projects provider payloads directly.
export async function executeTurn(
  request: ChatRequest,
  context: TurnExecutionContext,
): Promise<readonly ApplicationEvent[]> {
  try {
    const response = await sendChat(request);
    return [
      {
        type: "message_completed",
        data: {
          messageId: context.assistantMessageId,
          model: response.model,
          content: response.content,
          finishReason: response.finishReason,
          usage: response.usage,
        },
      },
      {
        type: "turn_receipt",
        data: {
          turnId: context.turnId,
          outcome: "completed",
          summary: "Provider response completed.",
        },
      },
    ];
  } catch (error) {
    const message =
      error instanceof Error
        ? error.message
        : "The selected provider could not complete this request.";
    return [
      {
        type: "message_completed",
        data: {
          messageId: context.assistantMessageId,
          model: request.model,
          content: message,
          finishReason: "error",
          usage: {},
        },
      },
      {
        type: "turn_receipt",
        data: {
          turnId: context.turnId,
          outcome: "failed",
          summary: message,
        },
      },
    ];
  }
}
