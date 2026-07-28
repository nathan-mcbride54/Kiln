export type ProviderId = "openai" | "anthropic" | "local";
export type ProviderState = "ready" | "untested" | "testing" | "error";
export type ChatRole = "system" | "developer" | "user" | "assistant";

export interface ProviderCredentials {
  apiKey?: string;
  organization?: string;
  project?: string;
  customHeaders?: Record<string, string>;
}

export interface ProviderConfig {
  id: ProviderId;
  name: string;
  shortName: string;
  protocol: string;
  description: string;
  baseUrl: string;
  model: string;
  apiKey: string;
  apiKeyRequired: boolean;
  state: ProviderState;
  accent: string;
  latency?: number;
  message?: string;
}

export interface ProviderCapabilities {
  provider: ProviderId;
  displayName: string;
  protocol:
    | "open_ai_responses"
    | "anthropic_messages"
    | "open_ai_chat_completions";
  defaultBaseUrl: string;
  apiKeyRequired: boolean;
  customBaseUrl: boolean;
  customHeaders: boolean;
  modelDiscovery: boolean;
  streaming: boolean;
  systemMessages: boolean;
  temperature: boolean;
}

export interface ConnectionTestResponse {
  provider: ProviderId;
  connected: boolean;
  latencyMs: number;
  discoveredModels?: number;
  message: string;
}

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

export interface ChatRequest {
  provider: ProviderId;
  credentials: ProviderCredentials;
  baseUrl?: string;
  model: string;
  messages: ChatMessage[];
  maxOutputTokens?: number;
  temperature?: number;
}

export interface ChatResponse {
  provider: ProviderId;
  id?: string;
  model: string;
  content: string;
  finishReason?: string;
  usage: {
    inputTokens?: number;
    outputTokens?: number;
    totalTokens?: number;
  };
}

export interface CommandError {
  code?: string;
  provider?: ProviderId;
  message?: string;
  retryable?: boolean;
}
