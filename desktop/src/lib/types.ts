export type ProviderId = "openai" | "anthropic" | "local";
export type ProviderState = "ready" | "untested" | "testing" | "error";
export type ChatRole = "system" | "developer" | "user" | "assistant";
export type CredentialBackend =
  | "windows_credential_manager"
  | "linux_secret_service";

export interface ProviderCredentialProfile {
  provider: ProviderId;
  credentialRef: string;
  backend: CredentialBackend;
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
  credentialRef?: string;
  credentialBackend?: CredentialBackend;
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
  credentialRef?: string;
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

export type ChatStreamEvent =
  | { type: "message_delta"; data: { delta: string } }
  | { type: "message_completed"; data: { response: ChatResponse } }
  | { type: "cancelled"; data: { reason: string } };

export interface CommandError {
  code?: string;
  provider?: ProviderId;
  message?: string;
  retryable?: boolean;
}

export interface ProjectDefaults {
  provider?: ProviderId;
  model?: string;
  verificationProfile?: string;
}

export interface RepositoryStatus {
  staged: number;
  modified: number;
  untracked: number;
  conflicts: number;
  ahead: number;
  behind: number;
}

export interface ProjectSnapshot {
  projectId: string;
  displayName: string;
  root: string;
  branch?: string;
  head?: string;
  status: RepositoryStatus;
  defaults: ProjectDefaults;
}

export interface RememberedProject {
  project: ProjectSnapshot;
  lastOpenedAtMs: number;
  available: boolean;
  unavailableReason?: string;
}

export type RepositoryToolRequest =
  | {
      tool: "read_file";
      input: {
        path: string;
        startLine?: number;
        lineCount?: number;
      };
    }
  | {
      tool: "search_files";
      input: {
        pattern: string;
        maxResults?: number;
      };
    }
  | {
      tool: "search_text";
      input: {
        query: string;
        path?: string;
        caseSensitive?: boolean;
        maxResults?: number;
      };
    }
  | {
      tool: "write_file";
      input: {
        path: string;
        content: string;
        expectedSha256?: string;
      };
    };

export interface FileMatch {
  path: string;
}

export interface TextMatch {
  path: string;
  line: number;
  column: number;
  preview: string;
}

export type RepositoryToolResult =
  | {
      tool: "read_file";
      result: {
        path: string;
        content: string;
        startLine: number;
        endLine: number;
        truncated: boolean;
        sha256: string;
      };
    }
  | {
      tool: "search_files";
      result: {
        pattern: string;
        matches: FileMatch[];
        truncated: boolean;
      };
    }
  | {
      tool: "search_text";
      result: {
        query: string;
        matches: TextMatch[];
        filesSearched: number;
        truncated: boolean;
      };
    }
  | {
      tool: "write_file";
      result: {
        path: string;
        created: boolean;
        bytesWritten: number;
        beforeSha256?: string;
        afterSha256: string;
        unifiedDiff: string;
      };
    };

export interface RepositoryToolExecution {
  result: RepositoryToolResult;
  activitySummary: string;
}
