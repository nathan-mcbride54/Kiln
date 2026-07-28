import { invoke } from "@tauri-apps/api/core";
import type {
  ChatRequest,
  ChatResponse,
  CommandError,
  ConnectionTestResponse,
  ProviderCapabilities,
  ProviderConfig,
} from "./types";

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
    streaming: false,
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
    streaming: false,
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
    streaming: false,
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

export function sendChat(request: ChatRequest): Promise<ChatResponse> {
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
