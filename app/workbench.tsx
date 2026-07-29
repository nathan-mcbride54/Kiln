"use client";

import {
  type CSSProperties,
  FormEvent,
  KeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  roadmap,
  roadmapCurrentHorizon,
  roadmapLastReviewed,
  roadmapRevision,
} from "./roadmap.generated";

type ProviderId = "openai" | "anthropic" | "local";
type ViewId = "workbench" | "providers" | "roadmap";
type TaskStatus = "running" | "review" | "done" | "paused";
type ProviderStatus =
  | "idle"
  | "testing"
  | "verifying"
  | "connected"
  | "limited"
  | "error";
type ProbeKey =
  | "reachability"
  | "authentication"
  | "modelDiscovery"
  | "streaming"
  | "toolCompatibility";
type ProbeStatus =
  | "passed"
  | "failed"
  | "unsupported"
  | "inconclusive"
  | "not_run";

type ProbeResult = {
  key: ProbeKey;
  label: string;
  status: ProbeStatus;
  message: string;
  latencyMs?: number;
  discoveredModels?: number;
};

type DiagnosticReport = {
  provider: ProviderId;
  origin: string;
  model?: string;
  probes: ProbeResult[];
  capabilities: {
    modelDiscovery: boolean;
    streaming: boolean;
    toolCalling: boolean;
  };
};

type Task = {
  id: string;
  title: string;
  repo: string;
  branch: string;
  provider: ProviderId;
  status: TaskStatus;
  updated: string;
  unread?: number;
};

type Message = {
  id: string;
  role: "user" | "assistant";
  body: string;
  label?: string;
};

type ProviderConfig = {
  model: string;
  endpoint: string;
  apiKey: string;
  status: ProviderStatus;
  statusText: string;
  report?: DiagnosticReport;
  credentialOrigin?: string;
  originWarning?: string;
};

const PROBE_NAME = "kiln_capability_probe";
const LOCAL_BODY_LIMIT = 256 * 1024;
const PROBE_KEYS: ProbeKey[] = [
  "reachability",
  "authentication",
  "modelDiscovery",
  "streaming",
  "toolCompatibility",
];
const PROBE_LABELS: Record<ProbeKey, string> = {
  reachability: "Reachability",
  authentication: "Authentication",
  modelDiscovery: "Model discovery",
  streaming: "Streaming",
  toolCompatibility: "Tool compatibility",
};

const providerMeta: Record<
  ProviderId,
  { name: string; short: string; color: string; privacy: string }
> = {
  openai: {
    name: "OpenAI",
    short: "OA",
    color: "#82d7b8",
    privacy: "Remote",
  },
  anthropic: {
    name: "Anthropic",
    short: "AN",
    color: "#f2a06b",
    privacy: "Remote",
  },
  local: {
    name: "Local server",
    short: "LO",
    color: "#b7a5ff",
    privacy: "On device",
  },
};

const initialTasks: Task[] = [
  {
    id: "command-palette",
    title: "Command palette",
    repo: "kiln-desktop",
    branch: "feat/command-palette",
    provider: "openai",
    status: "review",
    updated: "2m",
    unread: 3,
  },
  {
    id: "provider-health",
    title: "Provider health checks",
    repo: "kiln-core",
    branch: "feat/provider-health",
    provider: "anthropic",
    status: "running",
    updated: "now",
  },
  {
    id: "restore-session",
    title: "Restore interrupted session",
    repo: "kiln-core",
    branch: "fix/session-recovery",
    provider: "local",
    status: "done",
    updated: "1h",
  },
  {
    id: "linux-pty",
    title: "Linux PTY adapter",
    repo: "kiln-platform",
    branch: "spike/linux-pty",
    provider: "local",
    status: "paused",
    updated: "1d",
  },
];

const initialMessages: Message[] = [
  {
    id: "m1",
    role: "user",
    body: "Add a keyboard-first command palette. It should search tasks, repositories, providers, and actions without leaking provider-specific behavior into the UI.",
  },
  {
    id: "m2",
    role: "assistant",
    label: "Plan accepted · 4 steps",
    body: "I’ll introduce a typed command registry, add fuzzy ranking behind the application boundary, wire the palette to project and task actions, then verify keyboard and screen-reader behavior.",
  },
];

const activity = [
  {
    time: "10:42:08",
    kind: "read",
    title: "Inspected command registry",
    detail: "src/lib/commands.ts · 184 lines",
  },
  {
    time: "10:42:11",
    kind: "plan",
    title: "Created implementation plan",
    detail: "4 steps · no approval required",
  },
  {
    time: "10:42:24",
    kind: "edit",
    title: "Added palette state machine",
    detail: "3 files · +142 −18",
  },
  {
    time: "10:42:31",
    kind: "test",
    title: "Ran focused checks",
    detail: "18 passed · 0 failed · 1.8s",
  },
];

const diffLines = [
  { type: "meta", text: "@@ -18,6 +18,18 @@" },
  { type: "same", text: " export type Command = {" },
  { type: "same", text: "   id: string;" },
  { type: "add", text: "+  keywords: string[];" },
  { type: "add", text: "+  available(context): boolean;" },
  { type: "same", text: "   run(): Promise<void>;" },
  { type: "same", text: " };" },
  { type: "add", text: "+" },
  { type: "add", text: "+export function rankCommands(" },
  { type: "add", text: "+  query: string," },
  { type: "add", text: "+  commands: Command[]," },
  { type: "add", text: "+): Command[] {" },
  { type: "add", text: "+  return fuzzyRank(query, commands);" },
  { type: "add", text: "+}" },
];

const defaultConfigs: Record<ProviderId, ProviderConfig> = {
  openai: {
    model: "gpt-5.6-terra",
    endpoint: "https://api.openai.com/v1",
    apiKey: "",
    status: "idle",
    statusText: "Session key required",
  },
  anthropic: {
    model: "claude-sonnet-4-8",
    endpoint: "https://api.anthropic.com/v1",
    apiKey: "",
    status: "idle",
    statusText: "Session key required",
  },
  local: {
    model: "qwen3-coder",
    endpoint: "http://127.0.0.1:11434/v1",
    apiKey: "",
    status: "idle",
    statusText: "Ready to test",
  },
};

const currentRoadmapPhase =
  roadmap.find((phase) => phase.id === roadmapCurrentHorizon) ?? roadmap[0];

function diagnosticProbe(
  key: ProbeKey,
  status: ProbeStatus,
  message: string,
  extra: Pick<ProbeResult, "latencyMs" | "discoveredModels"> = {},
): ProbeResult {
  return {
    key,
    label: PROBE_LABELS[key],
    status,
    message,
    ...extra,
  };
}

function blankReport(
  provider: ProviderId,
  origin: string,
  model?: string,
): DiagnosticReport {
  const probes = PROBE_KEYS.map((key) =>
    diagnosticProbe(
      key,
      "not_run",
      key === "streaming" || key === "toolCompatibility"
        ? "Use Verify streaming & tools to run this model probe."
        : "Run the basic connection test.",
    ),
  );
  return {
    provider,
    origin,
    model: model?.trim() || undefined,
    probes,
    capabilities: {
      modelDiscovery: false,
      streaming: false,
      toolCalling: false,
    },
  };
}

function withProbe(
  report: DiagnosticReport,
  replacement: ProbeResult,
): DiagnosticReport {
  const probes = report.probes.map((item) =>
    item.key === replacement.key ? replacement : item,
  );
  const status = (key: ProbeKey) =>
    probes.find((item) => item.key === key)?.status;
  return {
    ...report,
    probes,
    capabilities: {
      modelDiscovery: status("modelDiscovery") === "passed",
      streaming: status("streaming") === "passed",
      toolCalling: status("toolCompatibility") === "passed",
    },
  };
}

function reportProbe(
  report: DiagnosticReport | undefined,
  key: ProbeKey,
): ProbeResult {
  return (
    report?.probes.find((item) => item.key === key) ??
    diagnosticProbe(
      key,
      "not_run",
      key === "streaming" || key === "toolCompatibility"
        ? "Run the explicit model verification."
        : "Run the basic connection test.",
    )
  );
}

function normalizedLocalEndpoint(
  value: string,
):
  | { baseUrl: string; origin: string; credentialsAllowed: boolean }
  | undefined {
  try {
    const parsed = new URL(value.trim());
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return undefined;
    }
    if (
      parsed.username ||
      parsed.password ||
      parsed.search ||
      parsed.hash
    ) {
      return undefined;
    }
    const baseUrl = parsed.toString().replace(/\/+$/, "");
    const hostname = parsed.hostname
      .toLowerCase()
      .replace(/^\[/, "")
      .replace(/\]$/, "");
    const ipv4 = hostname.split(".");
    const loopback =
      hostname === "localhost" ||
      hostname === "::1" ||
      (ipv4.length === 4 &&
        ipv4.every(
          (part) =>
            /^\d{1,3}$/.test(part) &&
            Number(part) >= 0 &&
            Number(part) <= 255,
        ) &&
        Number(ipv4[0]) === 127);
    return {
      baseUrl,
      origin: parsed.origin,
      credentialsAllowed: parsed.protocol === "https:" || loopback,
    };
  } catch {
    return undefined;
  }
}

async function localFetchOnce(
  url: string,
  init: RequestInit,
  timeoutMs: number,
): Promise<Response> {
  return fetch(url, {
    ...init,
    cache: "no-store",
    redirect: "manual",
    signal: AbortSignal.timeout(timeoutMs),
  });
}

function isLocalTimeoutError(cause: unknown) {
  return (
    cause instanceof Error &&
    (cause.name === "AbortError" || cause.name === "TimeoutError")
  );
}

async function readLocalBody(response: Response): Promise<string> {
  if (!response.body) return "";
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let total = 0;
  let text = "";
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > LOCAL_BODY_LIMIT) {
      await reader.cancel();
      throw new Error("body_limit");
    }
    text += decoder.decode(value, { stream: true });
  }
  return text + decoder.decode();
}

function safeJson(text: string): unknown | undefined {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

function localModelCount(payload: unknown): number | undefined {
  if (!payload || typeof payload !== "object") return undefined;
  const record = payload as { data?: unknown; models?: unknown };
  const source = Array.isArray(record.data)
    ? record.data
    : Array.isArray(record.models)
      ? record.models
      : Array.isArray(payload)
        ? payload
        : undefined;
  if (!source) return undefined;
  if (source.length === 0) return 0;
  return source.every((item) => {
    if (!item || typeof item !== "object") return false;
    const model = item as { id?: unknown; name?: unknown; model?: unknown };
    return [model.id, model.name, model.model].some(
      (value) => typeof value === "string" && value.trim().length > 0,
    );
  })
    ? source.length
    : undefined;
}

function parseLocalSse(text: string) {
  const events: unknown[] = [];
  let done = false;
  for (const block of text.split(/\r?\n\r?\n/)) {
    const data = block
      .split(/\r?\n/)
      .filter((line) => line.startsWith("data:"))
      .map((line) => line.slice(5).trimStart())
      .join("\n")
      .trim();
    if (!data) continue;
    if (data === "[DONE]") {
      done = true;
      continue;
    }
    const parsed = safeJson(data);
    if (parsed !== undefined) events.push(parsed);
  }
  return { events, done };
}

function recursiveField(
  value: unknown,
  key: string,
  expected: string,
  depth = 0,
): boolean {
  if (depth > 7 || !value || typeof value !== "object") return false;
  if (Array.isArray(value)) {
    return value.some((item) =>
      recursiveField(item, key, expected, depth + 1),
    );
  }
  const record = value as Record<string, unknown>;
  if (record[key] === expected) return true;
  return Object.values(record).some((item) =>
    recursiveField(item, key, expected, depth + 1),
  );
}

function hasLocalTextDelta(event: unknown) {
  if (!event || typeof event !== "object") return false;
  const choices = (event as { choices?: unknown }).choices;
  if (!Array.isArray(choices)) return false;
  return choices.some((choice) => {
    if (!choice || typeof choice !== "object") return false;
    const delta = (choice as { delta?: unknown }).delta;
    return (
      !!delta &&
      typeof delta === "object" &&
      typeof (delta as { content?: unknown }).content === "string"
    );
  });
}

function collectLocalToolEvidence(
  value: unknown,
  names: string[],
  argumentsList: string[],
  depth = 0,
) {
  if (depth > 7 || !value || typeof value !== "object") return;
  if (Array.isArray(value)) {
    value.forEach((item) =>
      collectLocalToolEvidence(item, names, argumentsList, depth + 1),
    );
    return;
  }
  const record = value as Record<string, unknown>;
  if (record.function && typeof record.function === "object") {
    const fn = record.function as Record<string, unknown>;
    if (typeof fn.name === "string") names.push(fn.name);
    if (typeof fn.arguments === "string") argumentsList.push(fn.arguments);
  }
  Object.values(record).forEach((item) =>
    collectLocalToolEvidence(item, names, argumentsList, depth + 1),
  );
}

function validLocalProbeArguments(values: string[]) {
  const valid = (value: string) => {
    const parsed = safeJson(value);
    return (
      !!parsed &&
      typeof parsed === "object" &&
      !Array.isArray(parsed) &&
      Object.keys(parsed).length === 1 &&
      (parsed as { value?: unknown }).value === "ok"
    );
  };
  return values.some(valid) || valid(values.join(""));
}

async function runLocalInferenceProbe(
  config: ProviderConfig,
  baseUrl: string,
  kind: "streaming" | "toolCompatibility",
): Promise<ProbeResult> {
  const key = kind;
  const started = Date.now();
  const headers = {
    Accept: "text/event-stream",
    "Content-Type": "application/json",
    ...(config.apiKey
      ? { Authorization: `Bearer ${config.apiKey}` }
      : {}),
  };
  const toolParameters = {
    type: "object",
    properties: { value: { type: "string", enum: ["ok"] } },
    required: ["value"],
    additionalProperties: false,
  };
  const payload =
    kind === "streaming"
      ? {
          model: config.model.trim(),
          messages: [{ role: "user", content: "Reply only with OK." }],
          max_tokens: 8,
          stream: true,
        }
      : {
          model: config.model.trim(),
          messages: [
            {
              role: "user",
              content:
                "Call kiln_capability_probe once with value ok. Do not answer otherwise.",
            },
          ],
          max_tokens: 64,
          stream: true,
          tools: [
            {
              type: "function",
              function: {
                name: PROBE_NAME,
                description:
                  "Synthetic no-op used only to verify the provider protocol.",
                parameters: toolParameters,
              },
            },
          ],
          tool_choice: {
            type: "function",
            function: { name: PROBE_NAME },
          },
        };

  try {
    const response = await localFetchOnce(
      `${baseUrl}/chat/completions`,
      {
        method: "POST",
        headers,
        body: JSON.stringify(payload),
      },
      45_000,
    );
    const latencyMs = Date.now() - started;
    if (response.status >= 300 && response.status < 400) {
      await response.body?.cancel();
      return diagnosticProbe(
        key,
        "failed",
        "Kiln refused a redirect before forwarding the session key.",
        { latencyMs },
      );
    }
    if (!response.ok) {
      await response.body?.cancel();
      return diagnosticProbe(
        key,
        response.status === 408 ||
          response.status === 409 ||
          response.status === 429 ||
          response.status >= 500
          ? "inconclusive"
          : "failed",
        "The local model did not complete this capability probe.",
        { latencyMs },
      );
    }

    const parsed = parseLocalSse(await readLocalBody(response));
    if (kind === "streaming") {
      const terminal =
        parsed.done ||
        parsed.events.some(
          (event) =>
            recursiveField(event, "finish_reason", "stop") ||
            recursiveField(event, "finish_reason", "length"),
        );
      const compatible =
        terminal && parsed.events.some((event) => hasLocalTextDelta(event));
      return diagnosticProbe(
        key,
        compatible ? "passed" : "failed",
        compatible
          ? "The selected model completed a compatible event stream."
          : "The selected model did not complete Kiln's streaming contract.",
        { latencyMs },
      );
    }

    const names: string[] = [];
    const argumentsList: string[] = [];
    parsed.events.forEach((event) =>
      collectLocalToolEvidence(event, names, argumentsList),
    );
    const terminal =
      parsed.done ||
      parsed.events.some((event) =>
        recursiveField(event, "finish_reason", "tool_calls"),
      );
    const compatible =
      terminal &&
      names.includes(PROBE_NAME) &&
      validLocalProbeArguments(argumentsList);
    return diagnosticProbe(
      key,
      compatible ? "passed" : "failed",
      compatible
        ? "The model emitted a valid synthetic tool request. Nothing was executed."
        : "The model did not emit Kiln's required tool-call shape.",
      { latencyMs },
    );
  } catch (cause) {
    return diagnosticProbe(
      key,
      "inconclusive",
      cause instanceof Error && cause.message === "body_limit"
        ? "The model response exceeded the diagnostic safety limit."
        : isLocalTimeoutError(cause)
          ? "The local model probe timed out."
          : "The browser could not inspect this model response. The server may be offline or may not allow this page through CORS.",
      { latencyMs: Date.now() - started },
    );
  }
}

async function runLocalDiagnostics(
  config: ProviderConfig,
  verify: boolean,
): Promise<DiagnosticReport> {
  const destination = normalizedLocalEndpoint(config.endpoint);
  let report = blankReport(
    "local",
    destination?.origin ?? "Invalid local endpoint",
    config.model,
  );
  if (!destination) {
    return withProbe(
      report,
      diagnosticProbe(
        "reachability",
        "failed",
        "Enter an absolute HTTP or HTTPS endpoint without credentials, a query, or a fragment.",
      ),
    );
  }
  if (config.apiKey && !destination.credentialsAllowed) {
    report = withProbe(
      report,
      diagnosticProbe(
        "authentication",
        "failed",
        "Kiln only sends a key over HTTPS or to localhost, 127/8, or ::1. The key was not sent.",
      ),
    );
    return report;
  }
  if (
    config.apiKey &&
    config.credentialOrigin !== destination.origin
  ) {
    report = withProbe(
      report,
      diagnosticProbe(
        "authentication",
        "failed",
        "Re-enter the optional key to bind it to this origin. It was not sent.",
      ),
    );
    return report;
  }

  const started = Date.now();
  try {
    const response = await localFetchOnce(
      `${destination.baseUrl}/models`,
      {
        headers: config.apiKey
          ? { Authorization: `Bearer ${config.apiKey}` }
          : undefined,
      },
      15_000,
    );
    const latencyMs = Date.now() - started;
    report = withProbe(
      report,
      diagnosticProbe(
        "reachability",
        "passed",
        "The configured local origin responded.",
        { latencyMs },
      ),
    );

    if (response.status >= 300 && response.status < 400) {
      await response.body?.cancel();
      report = withProbe(
        report,
        diagnosticProbe(
          "authentication",
          "inconclusive",
          "Kiln refused a redirect before forwarding the session key.",
        ),
      );
      return withProbe(
        report,
        diagnosticProbe(
          "modelDiscovery",
          "failed",
          "Model discovery redirected away from the configured endpoint.",
        ),
      );
    }
    if (response.status === 401 || response.status === 403) {
      await response.body?.cancel();
      report = withProbe(
        report,
        diagnosticProbe(
          "authentication",
          "failed",
          config.apiKey
            ? "The optional session key was not accepted."
            : "This server requires a key. Enter it for this exact origin.",
        ),
      );
      return withProbe(
        report,
        diagnosticProbe(
          "modelDiscovery",
          "not_run",
          "Model discovery needs accepted credentials.",
        ),
      );
    }
    if (!response.ok) {
      await response.body?.cancel();
      report = withProbe(
        report,
        diagnosticProbe(
          "authentication",
          "inconclusive",
          "The server response did not verify whether a key is required.",
        ),
      );
      return withProbe(
        report,
        diagnosticProbe(
          "modelDiscovery",
          response.status === 404 || response.status === 405
            ? "unsupported"
            : "inconclusive",
          response.status === 404 || response.status === 405
            ? "This server did not expose a compatible model list."
            : "Model discovery could not be verified right now.",
        ),
      );
    }

    report = withProbe(
      report,
      diagnosticProbe(
        "authentication",
        "passed",
        config.apiKey
          ? "The session key was accepted."
          : "This endpoint accepted the request without a key.",
      ),
    );
    const discoveredModels = localModelCount(
      safeJson(await readLocalBody(response)),
    );
    report = withProbe(
      report,
      discoveredModels === undefined
        ? diagnosticProbe(
            "modelDiscovery",
            "failed",
            "The server returned a model list Kiln could not safely interpret.",
          )
        : diagnosticProbe(
            "modelDiscovery",
            "passed",
            discoveredModels === 1
              ? "Discovered 1 available model."
              : `Discovered ${discoveredModels} available models.`,
            { discoveredModels },
          ),
    );
  } catch (cause) {
    return withProbe(
      report,
      diagnosticProbe(
        "reachability",
        "inconclusive",
        cause instanceof Error && cause.message === "body_limit"
          ? "The local response exceeded the diagnostic safety limit."
          : isLocalTimeoutError(cause)
            ? "The local endpoint did not respond before the timeout."
            : "The browser could not inspect this origin. The server may be offline or may not allow this page through CORS.",
        { latencyMs: Date.now() - started },
      ),
    );
  }

  if (!verify) return report;
  const reachability = reportProbe(report, "reachability");
  const authentication = reportProbe(report, "authentication");
  if (
    reachability.status !== "passed" ||
    authentication.status === "failed"
  ) {
    return report;
  }
  if (!config.model.trim()) {
    report = withProbe(
      report,
      diagnosticProbe(
        "streaming",
        "not_run",
        "Choose a model before verification.",
      ),
    );
    return withProbe(
      report,
      diagnosticProbe(
        "toolCompatibility",
        "not_run",
        "Choose a model before verification.",
      ),
    );
  }

  report = withProbe(
    report,
    await runLocalInferenceProbe(
      config,
      destination.baseUrl,
      "streaming",
    ),
  );
  report = withProbe(
    report,
    await runLocalInferenceProbe(
      config,
      destination.baseUrl,
      "toolCompatibility",
    ),
  );
  if (
    authentication.status === "inconclusive" &&
    (reportProbe(report, "streaming").status === "passed" ||
      reportProbe(report, "toolCompatibility").status === "passed")
  ) {
    report = withProbe(
      report,
      diagnosticProbe(
        "authentication",
        "passed",
        "A model request was accepted at this origin.",
      ),
    );
  }
  return report;
}

function providerStatusForReport(report: DiagnosticReport): ProviderStatus {
  const reachability = reportProbe(report, "reachability").status;
  const authentication = reportProbe(report, "authentication").status;
  if (reachability === "failed" || authentication === "failed") return "error";
  if (reachability === "passed" && authentication === "passed") {
    return "connected";
  }
  return "limited";
}

function providerStatusText(report: DiagnosticReport) {
  if (
    report.capabilities.streaming &&
    report.capabilities.toolCalling
  ) {
    return "Streaming and tools verified";
  }
  const status = providerStatusForReport(report);
  if (status === "connected") return "Basic checks passed";
  if (status === "error") return "Connection needs attention";
  return "Browser verification is incomplete";
}

function probeDisplay(status: ProbeStatus) {
  switch (status) {
    case "passed":
      return "Verified";
    case "failed":
      return "Failed";
    case "unsupported":
      return "Not exposed";
    case "inconclusive":
      return "Unclear";
    default:
      return "Not tested";
  }
}

function StatusDot({ status }: { status: TaskStatus }) {
  return <span className={`status-dot status-${status}`} aria-hidden="true" />;
}

function ProviderMark({ id, small = false }: { id: ProviderId; small?: boolean }) {
  const meta = providerMeta[id];
  return (
    <span
      className={`provider-mark ${small ? "provider-mark-small" : ""}`}
      style={{ "--provider-color": meta.color } as CSSProperties}
      aria-hidden="true"
    >
      {meta.short}
    </span>
  );
}

export function KilnWorkbench() {
  const [view, setView] = useState<ViewId>("workbench");
  const [tasks, setTasks] = useState(initialTasks);
  const [activeTaskId, setActiveTaskId] = useState(initialTasks[0].id);
  const [provider, setProvider] = useState<ProviderId>("openai");
  const [configs, setConfigs] = useState(defaultConfigs);
  const [messages, setMessages] = useState(initialMessages);
  const [prompt, setPrompt] = useState("");
  const [running, setRunning] = useState(false);
  const [toast, setToast] = useState("");
  const composerRef = useRef<HTMLTextAreaElement>(null);

  const activeTask = useMemo(
    () => tasks.find((task) => task.id === activeTaskId) ?? tasks[0],
    [activeTaskId, tasks],
  );

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(""), 3200);
    return () => window.clearTimeout(timer);
  }, [toast]);

  function updateConfig(id: ProviderId, patch: Partial<ProviderConfig>) {
    setConfigs((current) => ({
      ...current,
      [id]: { ...current[id], ...patch },
    }));
  }

  function changeModel(id: ProviderId, model: string) {
    updateConfig(id, {
      model,
      report: undefined,
      status: "idle",
      statusText: "Model changed — run checks",
    });
  }

  function changeEndpoint(endpoint: string) {
    setConfigs((current) => {
      const config = current.local;
      const destination = normalizedLocalEndpoint(endpoint);
      const changedCredentialDestination =
        !!config.apiKey &&
        !!destination &&
        config.credentialOrigin !== destination.origin;
      const originWarning = changedCredentialDestination
        ? `Destination changed from ${
            config.credentialOrigin ?? "an invalid destination"
          } to ${destination.origin}. The previous key was cleared. Re-enter it to create a new session binding.`
        : config.originWarning && destination
          ? `The previous key was cleared after a destination change. Re-enter it to bind a new session key to ${destination.origin}.`
          : config.originWarning;

      return {
        ...current,
        local: {
          ...config,
          endpoint,
          apiKey: changedCredentialDestination ? "" : config.apiKey,
          credentialOrigin: changedCredentialDestination
            ? undefined
            : config.credentialOrigin,
          originWarning,
          report: undefined,
          status: "idle",
          statusText: changedCredentialDestination
            ? "Destination changed — re-enter key"
            : "Ready to test",
        },
      };
    });
  }

  function changeApiKey(id: ProviderId, apiKey: string) {
    if (id !== "local") {
      updateConfig(id, {
        apiKey,
        report: undefined,
        status: "idle",
        statusText: apiKey.trim() ? "Ready to test" : "Session key required",
      });
      return;
    }

    setConfigs((current) => {
      const config = current.local;
      const destination = normalizedLocalEndpoint(config.endpoint);
      const hasKey = apiKey.length > 0;
      const credentialsAllowed = destination?.credentialsAllowed ?? false;
      return {
        ...current,
        local: {
          ...config,
          apiKey,
          credentialOrigin:
            hasKey && credentialsAllowed ? destination?.origin : undefined,
          originWarning:
            hasKey && !destination
              ? "Fix the local endpoint, then re-enter the key so Kiln can bind it to the exact destination."
              : hasKey && !credentialsAllowed
                ? "Kiln only sends a key over HTTPS or to localhost, 127/8, or ::1. This key remains in memory and will not be sent."
              : undefined,
          report: undefined,
          status: "idle",
          statusText:
            hasKey && (!destination || !credentialsAllowed)
              ? "Key blocked for this endpoint"
              : "Ready to test",
        },
      };
    });
  }

  async function runProviderDiagnostics(id: ProviderId, verify: boolean) {
    const config = configs[id];
    updateConfig(id, {
      status: verify ? "verifying" : "testing",
      statusText: verify
        ? "Verifying model capabilities…"
        : "Running basic checks…",
    });

    try {
      let report: DiagnosticReport;
      if (id === "local") {
        report = await runLocalDiagnostics(config, verify);
      } else {
        const response = await fetch("/api/provider", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            action: verify ? "verify" : "test",
            provider: id,
            model: config.model,
            apiKey: config.apiKey,
          }),
        });
        const result = (await response.json()) as {
          ok?: boolean;
          error?: string;
          report?: DiagnosticReport;
        };
        if (!response.ok || !result.ok || !result.report) {
          throw new Error(result.error || "The provider checks could not run.");
        }
        report = result.report;
      }

      const status = providerStatusForReport(report);
      updateConfig(id, {
        report,
        status,
        statusText: providerStatusText(report),
      });
      setToast(
        `${providerMeta[id].name} ${
          verify ? "capability verification" : "connection checks"
        } finished`,
      );
    } catch (error) {
      updateConfig(id, {
        status: "error",
        statusText:
          error instanceof Error
            ? error.message
            : "The provider checks could not run.",
      });
    }
  }

  async function testProvider(id: ProviderId) {
    await runProviderDiagnostics(id, false);
  }

  async function verifyProvider(id: ProviderId) {
    await runProviderDiagnostics(id, true);
  }

  async function submitPrompt(event?: FormEvent) {
    event?.preventDefault();
    const body = prompt.trim();
    if (!body || running) return;

    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      body,
    };
    setMessages((current) => [...current, userMessage]);
    setPrompt("");
    setRunning(true);
    setTasks((current) =>
      current.map((task) =>
        task.id === activeTaskId
          ? { ...task, status: "running", updated: "now" }
          : task,
      ),
    );

    try {
      const config = configs[provider];
      let assistantText = "";

      if (provider === "local") {
        const destination = normalizedLocalEndpoint(config.endpoint);
        if (!destination) {
          throw new Error(
            "Enter a valid local HTTP or HTTPS endpoint before sending.",
          );
        }
        if (config.apiKey && !destination.credentialsAllowed) {
          throw new Error(
            "Kiln only sends a key over HTTPS or to localhost, 127/8, or ::1.",
          );
        }
        if (
          config.apiKey &&
          config.credentialOrigin !== destination.origin
        ) {
          throw new Error(
            "The local destination changed. Re-enter the optional key before sending it.",
          );
        }
        const response = await localFetchOnce(
          `${destination.baseUrl}/chat/completions`,
          {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            ...(config.apiKey
              ? { Authorization: `Bearer ${config.apiKey}` }
              : {}),
          },
          body: JSON.stringify({
            model: config.model,
            stream: false,
            messages: [...messages, userMessage].map((message) => ({
              role: message.role,
              content: message.body,
            })),
          }),
          },
          60_000,
        );
        if (response.status >= 300 && response.status < 400) {
          await response.body?.cancel();
          throw new Error(
            "Kiln refused a redirect before forwarding the session key.",
          );
        }
        if (!response.ok) {
          await response.body?.cancel();
          throw new Error(`Local server returned ${response.status}`);
        }
        const result = safeJson(await readLocalBody(response)) as
          | {
          choices?: Array<{ message?: { content?: string } }>;
            }
          | undefined;
        assistantText =
          result?.choices?.[0]?.message?.content ??
          "The local server returned no text.";
      } else if (config.apiKey && config.status === "connected") {
        const response = await fetch("/api/provider", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            action: "chat",
            provider,
            model: config.model,
            apiKey: config.apiKey,
            messages: [...messages, userMessage].map((message) => ({
              role: message.role,
              content: message.body,
            })),
          }),
        });
        const result = (await response.json()) as {
          text?: string;
          error?: string;
        };
        if (!response.ok) {
          throw new Error(result.error || "Provider request failed");
        }
        assistantText = result.text || "The provider returned no text.";
      } else {
        await new Promise((resolve) => window.setTimeout(resolve, 850));
        assistantText =
          "I’m in preview mode, so I mapped this request into a safe execution plan without touching your repository. Connect this provider to run the same turn live; credentials remain in memory for this session only.";
      }

      setMessages((current) => [
        ...current,
        {
          id: crypto.randomUUID(),
          role: "assistant",
          label:
            configs[provider].status === "connected"
              ? `${providerMeta[provider].name} · completed`
              : "Preview plan · no files changed",
          body: assistantText,
        },
      ]);
      setTasks((current) =>
        current.map((task) =>
          task.id === activeTaskId
            ? { ...task, status: "review", updated: "now", unread: 1 }
            : task,
        ),
      );
    } catch (error) {
      setToast(
        error instanceof Error ? error.message : "The provider request failed",
      );
      setTasks((current) =>
        current.map((task) =>
          task.id === activeTaskId
            ? { ...task, status: "paused", updated: "now" }
            : task,
        ),
      );
    } finally {
      setRunning(false);
    }
  }

  function composerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void submitPrompt();
    }
  }

  function chooseTask(id: string) {
    const nextTask = tasks.find((task) => task.id === id);
    setActiveTaskId(id);
    if (nextTask) setProvider(nextTask.provider);
    setTasks((current) =>
      current.map((task) =>
        task.id === id ? { ...task, unread: undefined } : task,
      ),
    );
    setView("workbench");
  }

  return (
    <main className="kiln-shell">
      <a href="#main-content" className="skip-link">
        Skip to workspace
      </a>

      <aside className="app-rail" aria-label="Primary navigation">
        <div className="brand-lockup">
          <span className="brand-glyph" aria-hidden="true">
            K
          </span>
          <span className="brand-name">kiln</span>
          <span className="alpha-tag">alpha</span>
        </div>

        <nav className="rail-nav" aria-label="Views">
          <button
            className={view === "workbench" ? "active" : ""}
            onClick={() => setView("workbench")}
          >
            <span className="nav-icon">⌘</span>
            Workbench
          </button>
          <button
            className={view === "providers" ? "active" : ""}
            onClick={() => setView("providers")}
          >
            <span className="nav-icon">◌</span>
            Providers
            <span className="nav-count">3</span>
          </button>
          <button
            className={view === "roadmap" ? "active" : ""}
            onClick={() => setView("roadmap")}
          >
            <span className="nav-icon">↗</span>
            Roadmap
          </button>
        </nav>

        <div className="rail-section-heading">
          <span>Tasks</span>
          <button
            aria-label="Create task"
            onClick={() => {
              setView("workbench");
              composerRef.current?.focus();
            }}
          >
            +
          </button>
        </div>

        <div className="task-list">
          {tasks.map((task) => (
            <button
              key={task.id}
              className={`task-row ${
                activeTaskId === task.id && view === "workbench" ? "active" : ""
              }`}
              onClick={() => chooseTask(task.id)}
            >
              <span className="task-status-wrap">
                <StatusDot status={task.status} />
              </span>
              <span className="task-copy">
                <span className="task-title">{task.title}</span>
                <span className="task-meta">
                  {task.repo} · {task.updated}
                </span>
              </span>
              {task.unread ? (
                <span className="unread-badge">{task.unread}</span>
              ) : (
                <ProviderMark id={task.provider} small />
              )}
            </button>
          ))}
        </div>

        <div className="rail-footer">
          <button className="repo-picker">
            <span className="repo-monogram">KD</span>
            <span>
              <strong>kiln-desktop</strong>
              <small>4 tasks · clean</small>
            </span>
            <span aria-hidden="true">⌄</span>
          </button>
          <div className="platform-row">
            <span className="platform-ready">Windows</span>
            <span className="platform-ready">Linux</span>
            <span>macOS later</span>
          </div>
        </div>
      </aside>

      {view === "workbench" && (
        <section className="workspace-view" id="main-content">
          <header className="workspace-header">
            <div>
              <div className="eyebrow-row">
                <span>{activeTask.repo}</span>
                <span>/</span>
                <span className="branch-name">{activeTask.branch}</span>
              </div>
              <h1>{activeTask.title}</h1>
            </div>
            <div className="header-actions">
              <span className="worktree-pill">
                <span className="pulse-dot" />
                isolated worktree
              </span>
              <button
                className="icon-button"
                aria-label="Open task menu"
                onClick={() => setToast("Task actions are ready")}
              >
                ···
              </button>
            </div>
          </header>

          <div className="workbench-grid">
            <section className="conversation-pane" aria-label="Conversation">
              <div className="conversation-scroll">
                <div className="turn-date">
                  <span>Today</span>
                </div>
                {messages.map((message) => (
                  <article
                    key={message.id}
                    className={`message message-${message.role}`}
                  >
                    <div className="message-avatar">
                      {message.role === "user" ? "NM" : "K"}
                    </div>
                    <div className="message-content">
                      <div className="message-heading">
                        <strong>
                          {message.role === "user" ? "You" : "Kiln"}
                        </strong>
                        <span>{message.role === "user" ? "10:42" : "10:43"}</span>
                      </div>
                      {message.label && (
                        <div className="message-label">
                          <span>✓</span>
                          {message.label}
                        </div>
                      )}
                      <p>{message.body}</p>
                    </div>
                  </article>
                ))}

                <div className="activity-cluster">
                  <div className="cluster-heading">
                    <span className="cluster-line" />
                    <span>Activity</span>
                    <span className="cluster-count">4 events</span>
                    <span className="cluster-line" />
                  </div>
                  {activity.map((event, index) => (
                    <div className="activity-event" key={event.title}>
                      <span className={`activity-icon activity-${event.kind}`}>
                        {event.kind === "read"
                          ? "R"
                          : event.kind === "plan"
                            ? "P"
                            : event.kind === "edit"
                              ? "E"
                              : "T"}
                      </span>
                      <span className="activity-track">
                        {index < activity.length - 1 && <i />}
                      </span>
                      <span className="activity-copy">
                        <strong>{event.title}</strong>
                        <small>{event.detail}</small>
                      </span>
                      <time>{event.time}</time>
                    </div>
                  ))}
                </div>

                {running && (
                  <div className="agent-working" role="status">
                    <span />
                    <span />
                    <span />
                    {providerMeta[provider].name} is working
                  </div>
                )}
              </div>

              <form className="composer" onSubmit={submitPrompt}>
                <div className="composer-topline">
                  <button
                    type="button"
                    className="provider-button"
                    onClick={() => setView("providers")}
                    aria-label="Configure current provider"
                  >
                    <ProviderMark id={provider} small />
                    <span>{providerMeta[provider].name}</span>
                    <small>{configs[provider].model}</small>
                    <span aria-hidden="true">⌄</span>
                  </button>
                  <span
                    className={`connection-chip connection-${configs[provider].status}`}
                  >
                    {configs[provider].status === "connected"
                      ? "live"
                      : "preview"}
                  </span>
                </div>
                <textarea
                  ref={composerRef}
                  value={prompt}
                  onChange={(event) => setPrompt(event.target.value)}
                  onKeyDown={composerKeyDown}
                  placeholder="Describe the outcome you want…"
                  aria-label="Message Kiln"
                  rows={3}
                />
                <div className="composer-footer">
                  <div>
                    <button
                      type="button"
                      className="composer-tool"
                      aria-label="Attach context"
                      onClick={() => setToast("Context picker is on the roadmap")}
                    >
                      +
                    </button>
                    <span>Smart approval</span>
                    <span className="privacy-label">
                      {providerMeta[provider].privacy}
                    </span>
                  </div>
                  <button
                    className="send-button"
                    disabled={!prompt.trim() || running}
                    type="submit"
                  >
                    {running ? "Running" : "Send"}
                    <span>⌘↵</span>
                  </button>
                </div>
              </form>
            </section>

            <aside className="inspector-pane" aria-label="Task inspector">
              <div className="inspector-tabs">
                <button className="active">Changes <span>3</span></button>
                <button>Plan</button>
                <button>Terminal</button>
              </div>
              <div className="change-summary">
                <div>
                  <strong>3 files changed</strong>
                  <span>
                    <b>+142</b>
                    <i>−18</i>
                  </span>
                </div>
                <div className="diff-bar">
                  <span style={{ width: "82%" }} />
                  <i style={{ width: "18%" }} />
                </div>
              </div>
              <div className="file-tree">
                <button className="active">
                  <span className="file-status">M</span>
                  <span>src/lib/commands.ts</span>
                  <small>+38 −4</small>
                </button>
                <button>
                  <span className="file-status file-added">A</span>
                  <span>src/lib/rank.ts</span>
                  <small>+62</small>
                </button>
                <button>
                  <span className="file-status">M</span>
                  <span>src/CommandPalette.svelte</span>
                  <small>+42 −14</small>
                </button>
              </div>
              <div className="diff-header">
                <span>commands.ts</span>
                <div>
                  <button aria-label="Previous change">↑</button>
                  <button aria-label="Next change">↓</button>
                  <button aria-label="More diff actions">···</button>
                </div>
              </div>
              <pre className="diff-view" aria-label="Code diff">
                {diffLines.map((line, index) => (
                  <code className={`diff-${line.type}`} key={`${line.text}-${index}`}>
                    <span>{index + 18}</span>
                    <b>{line.text}</b>
                  </code>
                ))}
              </pre>
              <div className="review-footer">
                <div>
                  <span className="checks-pass">✓</span>
                  <span>
                    <strong>Checks passed</strong>
                    <small>18 tests · 1.8s</small>
                  </span>
                </div>
                <button
                  onClick={() => setToast("Commit flow will remain explicit")}
                >
                  Review & commit
                </button>
              </div>
            </aside>
          </div>
        </section>
      )}

      {view === "providers" && (
        <section className="content-view provider-view" id="main-content">
          <header className="content-header">
            <div>
              <span className="content-kicker">Connections</span>
              <h1>Bring the model. Keep the control.</h1>
              <p>
                Cloud credentials live in memory for this session only. Local
                requests go directly from this device to your configured server.
              </p>
            </div>
            <div className="security-note">
              <span>◇</span>
              <div>
                <strong>Local-first credential boundary</strong>
                <small>Nothing is written to browser storage or transcripts.</small>
              </div>
            </div>
          </header>

          <div className="provider-grid">
            {(Object.keys(providerMeta) as ProviderId[]).map((id) => {
              const config = configs[id];
              const meta = providerMeta[id];
              const busy =
                config.status === "testing" || config.status === "verifying";
              return (
                <article
                  className={`provider-card provider-card-${config.status}`}
                  key={id}
                  style={{ "--provider-color": meta.color } as CSSProperties}
                >
                  <div className="provider-card-head">
                    <ProviderMark id={id} />
                    <div>
                      <h2>{meta.name}</h2>
                      <p>
                        {id === "openai"
                          ? "Responses API"
                          : id === "anthropic"
                            ? "Messages API"
                            : "OpenAI-compatible"}
                      </p>
                    </div>
                    <span className={`provider-status provider-status-${config.status}`}>
                      {config.status === "testing"
                        ? "Testing"
                        : config.status === "verifying"
                          ? "Verifying"
                        : config.status === "connected"
                          ? "Connected"
                          : config.status === "limited"
                            ? "Incomplete"
                          : config.status === "error"
                            ? "Needs attention"
                            : "Not connected"}
                    </span>
                  </div>

                  <label>
                    <span>Model</span>
                    <input
                      value={config.model}
                      onChange={(event) => changeModel(id, event.target.value)}
                      spellCheck={false}
                    />
                  </label>
                  <label>
                    <span>
                      Endpoint{" "}
                      <small>{id === "local" ? "custom" : "pinned by Kiln"}</small>
                    </span>
                    <input
                      value={config.endpoint}
                      disabled={id !== "local"}
                      onChange={(event) => changeEndpoint(event.target.value)}
                      spellCheck={false}
                    />
                  </label>
                  <label>
                    <span>
                      API key {id === "local" && <small>optional</small>}
                    </span>
                    <input
                      type="password"
                      value={config.apiKey}
                      placeholder={id === "local" ? "Optional bearer token" : "Session only"}
                      onChange={(event) => changeApiKey(id, event.target.value)}
                      autoComplete="off"
                    />
                  </label>
                  {config.originWarning && (
                    <div className="origin-warning" role="alert">
                      <strong>Credential boundary</strong>
                      <span>{config.originWarning}</span>
                    </div>
                  )}
                  <div
                    className="probe-list"
                    aria-label={`${meta.name} diagnostic results`}
                  >
                    {PROBE_KEYS.map((key) => {
                      const probe = reportProbe(config.report, key);
                      return (
                        <div
                          className={`probe-row probe-result-${probe.status}`}
                          title={probe.message}
                          key={key}
                        >
                          <span className="probe-indicator" aria-hidden="true" />
                          <span className="probe-copy">
                            <span>{probe.label}</span>
                            {config.report && <small>{probe.message}</small>}
                          </span>
                          <strong>{probeDisplay(probe.status)}</strong>
                        </div>
                      );
                    })}
                  </div>
                  <div className="provider-card-footer">
                    <div className="connection-summary">
                      <span
                        className={`connection-light connection-light-${config.status}`}
                      />
                      <span>{config.statusText}</span>
                    </div>
                    <div className="provider-actions">
                      <button
                        onClick={() => void testProvider(id)}
                        disabled={busy}
                      >
                        {config.report ? "Run basic test again" : "Test connection"}
                      </button>
                      <button
                        className="verify-button"
                        onClick={() => void verifyProvider(id)}
                        disabled={busy}
                      >
                        Verify streaming &amp; tools
                      </button>
                    </div>
                  </div>
                  <p className="verification-note">
                    The basic test does not generate text. Verification sends two
                    tiny synthetic prompts, which may use provider tokens or warm a
                    local model. Kiln never executes the synthetic tool.
                  </p>
                </article>
              );
            })}
          </div>

          <section className="capability-matrix">
            <div className="section-title-row">
              <div>
                <span className="content-kicker">Capability contract</span>
                <h2>The interface follows support, not brand names.</h2>
              </div>
              <span className="matrix-note">Live results from five independent probes</span>
            </div>
            <div className="matrix-table" role="table" aria-label="Provider capabilities">
              <div className="matrix-row matrix-head" role="row">
                <span>Capability</span>
                <span>OpenAI</span>
                <span>Anthropic</span>
                <span>Local</span>
              </div>
              {PROBE_KEYS.map((key) => (
                <div className="matrix-row" role="row" key={key}>
                  <strong>{PROBE_LABELS[key]}</strong>
                  {(["openai", "anthropic", "local"] as ProviderId[]).map(
                    (providerId) => {
                      const probe = reportProbe(configs[providerId].report, key);
                      return (
                        <span
                          className={`matrix-value probe-result-${probe.status}`}
                          title={probe.message}
                          key={providerId}
                        >
                          {probeDisplay(probe.status)}
                        </span>
                      );
                    },
                  )}
                </div>
              ))}
            </div>
          </section>
        </section>
      )}

      {view === "roadmap" && (
        <section className="content-view roadmap-view" id="main-content">
          <header className="content-header roadmap-hero">
            <div>
              <span className="content-kicker">
                Living roadmap · revision {roadmapRevision}
              </span>
              <h1>Earn autonomy one reliable layer at a time.</h1>
              <p>
                Every milestone is defined by a user outcome and acceptance
                gates—not by a pile of shipped toggles.
              </p>
            </div>
            <div className="launch-target">
              <span>Launch target</span>
              <strong>Windows + Linux</strong>
              <small>
                Reviewed {roadmapLastReviewed} · macOS follows release-gate parity
              </small>
            </div>
          </header>

          <div className="roadmap-overview">
            <div className="roadmap-stat">
              <span>Current horizon</span>
              <strong>{currentRoadmapPhase.id}</strong>
              <small>{currentRoadmapPhase.title}</small>
            </div>
            <div className="roadmap-stat">
              <span>Core promise</span>
              <strong>Visible agency</strong>
              <small>Plan → tools → tested diff</small>
            </div>
            <div className="roadmap-stat">
              <span>Architecture</span>
              <strong>Local first</strong>
              <small>Remote later, same core</small>
            </div>
          </div>

          <div className="roadmap-list">
            {roadmap.map((phase, index) => (
              <article className="roadmap-card" key={phase.id}>
                <div className="roadmap-line" aria-hidden="true">
                  <span className={phase.progress > 0 ? "filled" : ""}>
                    {phase.id}
                  </span>
                  {index < roadmap.length - 1 && <i />}
                </div>
                <div className="roadmap-card-body">
                  <div className="roadmap-card-head">
                    <div>
                      <span className={`phase-status phase-${phase.status.toLowerCase().replace(" ", "-")}`}>
                        {phase.status}
                      </span>
                      <h2>{phase.title}</h2>
                      <p>{phase.outcome}</p>
                    </div>
                    <strong className="phase-progress">{phase.progress}%</strong>
                  </div>
                  <div className="phase-progress-track">
                    <span style={{ width: `${Math.max(phase.progress, 2)}%` }} />
                  </div>
                  <div className="phase-columns">
                    <div>
                      <h3>Scope</h3>
                      <ul>
                        {phase.now.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </div>
                    <div>
                      <h3>Exit gates</h3>
                      <ul className="gate-list">
                        {phase.gates.map((gate) => (
                          <li key={gate}>
                            <span>◇</span>
                            {gate}
                          </li>
                        ))}
                      </ul>
                    </div>
                  </div>
                </div>
              </article>
            ))}
          </div>

          <section className="principles-panel">
            <div>
              <span className="content-kicker">Non-negotiables</span>
              <h2>The rules that keep Kiln honest.</h2>
            </div>
            <div className="principle-grid">
              {[
                ["01", "Local-first ownership", "Credentials, policy, and history stay on your machine."],
                ["02", "Provider freedom", "OpenAI, Anthropic, and local models are peers."],
                ["03", "Visible agency", "Every plan, tool, command, approval, and diff is inspectable."],
                ["04", "Recoverable by design", "Sessions survive restarts and checkpoints preserve choices."],
                ["05", "Protocol over parsing", "Typed APIs, ACP, and MCP replace terminal guesswork."],
                ["06", "Cross-platform first", "No Unix-only path, process, shell, or signal assumptions."],
              ].map(([number, title, description]) => (
                <div className="principle" key={number}>
                  <span>{number}</span>
                  <strong>{title}</strong>
                  <p>{description}</p>
                </div>
              ))}
            </div>
          </section>
        </section>
      )}

      <nav className="mobile-nav" aria-label="Mobile navigation">
        <button
          className={view === "workbench" ? "active" : ""}
          onClick={() => setView("workbench")}
        >
          Workbench
        </button>
        <button
          className={view === "providers" ? "active" : ""}
          onClick={() => setView("providers")}
        >
          Providers
        </button>
        <button
          className={view === "roadmap" ? "active" : ""}
          onClick={() => setView("roadmap")}
        >
          Roadmap
        </button>
      </nav>

      {toast && (
        <div className="toast" role="status">
          <span>✓</span>
          {toast}
        </div>
      )}
    </main>
  );
}
