import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ApplicationEvent,
  EventEnvelope,
} from "./events.ts";
import type {
  ChatRequest,
  ChatResponse,
  CommandError,
  ConnectionTestResponse,
  ProjectDefaults,
  ProjectSnapshot,
  ProviderCapabilities,
  ProviderConfig,
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
    systemMessages: true,
    temperature: true,
  },
];

export function isDesktopRuntime(): boolean {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

async function callOrPreview<T>(
  command: string,
  args: Record<string, unknown>,
  preview: () => Promise<T> | T,
): Promise<T> {
  if (!isDesktopRuntime()) {
    await wait(520);
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

    throw new Error(
      commandError.message ??
        `Kiln could not complete “${command.replaceAll("_", " ")}”.`,
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

export function testProviderConnection(
  provider: ProviderConfig,
): Promise<ConnectionTestResponse> {
  return callOrPreview(
    "test_connection",
    {
      request: {
        provider: provider.id,
        credentials: {
          apiKey: provider.apiKey || undefined,
        },
        baseUrl: provider.baseUrl || undefined,
      },
    },
    () => ({
      provider: provider.id,
      connected: true,
      latencyMs: provider.id === "local" ? 18 : provider.id === "openai" ? 84 : 112,
      discoveredModels: provider.id === "local" ? 4 : undefined,
      message: isDesktopRuntime()
        ? "Connection ready"
        : "Preview connection ready — launch the desktop app for a live test.",
    }),
  );
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
  await consume([
    {
      type: "tool_proposed",
      data: {
        toolCallId,
        name: request.tool,
        summary: repositoryToolProposalSummary(request),
      },
    },
    { type: "tool_started", data: { toolCallId } },
  ]);

  try {
    const execution = await executeRepositoryTool(
      projectId,
      toolCallId,
      request,
      turnId,
    );
    await consume([
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
    ]);
    return execution.result;
  } catch (error) {
    await consume([
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
    ]);
    throw error;
  }
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
