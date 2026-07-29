import { NextRequest, NextResponse } from "next/server";

type ProviderId = "openai" | "anthropic";
type InputMessage = { role: "user" | "assistant"; content: string };
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

type ProviderRequest = {
  action?: "test" | "verify" | "chat";
  provider?: ProviderId;
  model?: string;
  apiKey?: string;
  messages?: InputMessage[];
};

type SseParseResult = {
  events: unknown[];
  done: boolean;
};

const BODY_LIMIT = 256 * 1024;
const MODEL_LIMIT = 200;
const API_KEY_LIMIT = 8 * 1024;
const MESSAGE_LIMIT = 64;
const MESSAGE_CONTENT_LIMIT = 32 * 1024;
const DISCOVERY_TIMEOUT_MS = 15_000;
const INFERENCE_TIMEOUT_MS = 45_000;
const PROBE_NAME = "kiln_capability_probe";
const PROBE_LABELS: Record<ProbeKey, string> = {
  reachability: "Reachability",
  authentication: "Authentication",
  modelDiscovery: "Model discovery",
  streaming: "Streaming",
  toolCompatibility: "Tool compatibility",
};

const CLOUD = {
  openai: {
    name: "OpenAI",
    origin: "https://api.openai.com",
    modelsUrl: "https://api.openai.com/v1/models",
    generationUrl: "https://api.openai.com/v1/responses",
  },
  anthropic: {
    name: "Anthropic",
    origin: "https://api.anthropic.com",
    modelsUrl: "https://api.anthropic.com/v1/models",
    generationUrl: "https://api.anthropic.com/v1/messages",
  },
} as const;

export const dynamic = "force-dynamic";

class BodyTooLarge extends Error {}

function json(
  body: Record<string, unknown>,
  status = 200,
) {
  return NextResponse.json(body, {
    status,
    headers: {
      "Cache-Control": "no-store, max-age=0",
    },
  });
}

function error(message: string, status = 400) {
  return json({ ok: false, error: message }, status);
}

function probe(
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

function emptyProbes(): ProbeResult[] {
  return [
    probe("reachability", "not_run", "Run the basic connection test."),
    probe("authentication", "not_run", "Run the basic connection test."),
    probe("modelDiscovery", "not_run", "Run the basic connection test."),
    probe(
      "streaming",
      "not_run",
      "Use Verify streaming & tools to run this model probe.",
    ),
    probe(
      "toolCompatibility",
      "not_run",
      "Use Verify streaming & tools to run this model probe.",
    ),
  ];
}

function replaceProbe(
  probes: ProbeResult[],
  replacement: ProbeResult,
): ProbeResult[] {
  return probes.map((item) =>
    item.key === replacement.key ? replacement : item,
  );
}

function buildReport(
  provider: ProviderId,
  model: string | undefined,
  probes: ProbeResult[],
) {
  const status = (key: ProbeKey) =>
    probes.find((item) => item.key === key)?.status;
  return {
    provider,
    origin: CLOUD[provider].origin,
    model: model?.trim() || undefined,
    probes,
    capabilities: {
      modelDiscovery: status("modelDiscovery") === "passed",
      streaming: status("streaming") === "passed",
      toolCalling: status("toolCompatibility") === "passed",
    },
  };
}

function cloudHeaders(
  provider: ProviderId,
  apiKey: string,
  streaming = false,
): Record<string, string> {
  const headers: Record<string, string> = {
    Accept: streaming ? "text/event-stream" : "application/json",
    "Content-Type": "application/json",
  };
  if (provider === "openai") {
    headers.Authorization = `Bearer ${apiKey}`;
  } else {
    headers["x-api-key"] = apiKey;
    headers["anthropic-version"] = "2023-06-01";
  }
  return headers;
}

async function fetchOnce(
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

function isTimeoutError(cause: unknown) {
  return (
    cause instanceof Error &&
    (cause.name === "AbortError" || cause.name === "TimeoutError")
  );
}

async function readBoundedStream(
  body: ReadableStream<Uint8Array> | null,
  limit = BODY_LIMIT,
): Promise<string> {
  if (!body) return "";
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let total = 0;
  let text = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > limit) {
      await reader.cancel().catch(() => undefined);
      throw new BodyTooLarge();
    }
    text += decoder.decode(value, { stream: true });
  }
  return text + decoder.decode();
}

async function readBoundedText(
  response: Response,
  limit = BODY_LIMIT,
): Promise<string> {
  return readBoundedStream(response.body, limit);
}

function parseJson(text: string): unknown | undefined {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

function countModels(payload: unknown): number | undefined {
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
  const valid = source.every((item) => {
    if (!item || typeof item !== "object") return false;
    const model = item as { id?: unknown; name?: unknown; model?: unknown };
    return [model.id, model.name, model.model].some(
      (value) => typeof value === "string" && value.trim().length > 0,
    );
  });
  return valid ? source.length : undefined;
}

function isRedirect(status: number) {
  return status >= 300 && status < 400;
}

function transientStatus(status: number) {
  return status === 408 || status === 409 || status === 429 || status >= 500;
}

async function runDiscovery(
  provider: ProviderId,
  apiKey: string,
): Promise<ProbeResult[]> {
  let probes = emptyProbes();
  const cloud = CLOUD[provider];
  const started = Date.now();

  try {
    const response = await fetchOnce(
      cloud.modelsUrl,
      { headers: cloudHeaders(provider, apiKey) },
      DISCOVERY_TIMEOUT_MS,
    );
    const latencyMs = Date.now() - started;
    probes = replaceProbe(
      probes,
      probe(
        "reachability",
        "passed",
        `${cloud.name} responded at its pinned origin.`,
        { latencyMs },
      ),
    );

    if (isRedirect(response.status)) {
      await response.body?.cancel();
      probes = replaceProbe(
        probes,
        probe(
          "authentication",
          "inconclusive",
          "Kiln refused an unexpected redirect before forwarding credentials.",
        ),
      );
      return replaceProbe(
        probes,
        probe(
          "modelDiscovery",
          "failed",
          "Model discovery redirected away from the pinned origin.",
        ),
      );
    }

    if (response.status === 401 || response.status === 403) {
      await response.body?.cancel();
      probes = replaceProbe(
        probes,
        probe(
          "authentication",
          "failed",
          response.status === 401
            ? "The session key was not accepted."
            : "The session key does not have permission for this resource.",
        ),
      );
      return replaceProbe(
        probes,
        probe(
          "modelDiscovery",
          "not_run",
          "Model discovery needs an accepted session key.",
        ),
      );
    }

    if (!response.ok) {
      await response.body?.cancel();
      const status: ProbeStatus = transientStatus(response.status)
        ? "inconclusive"
        : response.status === 404 || response.status === 405
          ? "unsupported"
          : "failed";
      probes = replaceProbe(
        probes,
        probe(
          "authentication",
          "inconclusive",
          "The provider responded, but this response did not verify the key.",
        ),
      );
      return replaceProbe(
        probes,
        probe(
          "modelDiscovery",
          status,
          status === "unsupported"
            ? "This endpoint did not expose model discovery."
            : "Model discovery could not be verified right now.",
        ),
      );
    }

    probes = replaceProbe(
      probes,
      probe("authentication", "passed", "The session key was accepted."),
    );
    const payload = parseJson(await readBoundedText(response));
    const discoveredModels = countModels(payload);
    return replaceProbe(
      probes,
      discoveredModels === undefined
        ? probe(
            "modelDiscovery",
            "failed",
            "The provider returned a model list Kiln could not safely interpret.",
          )
        : probe(
            "modelDiscovery",
            "passed",
            discoveredModels === 1
              ? "Discovered 1 available model."
              : `Discovered ${discoveredModels} available models.`,
            { discoveredModels },
          ),
    );
  } catch (cause) {
    const message =
      cause instanceof BodyTooLarge
        ? "The provider response exceeded the diagnostic safety limit."
        : isTimeoutError(cause)
          ? "The provider did not respond before the diagnostic timeout."
          : "The pinned provider origin could not be reached.";
    probes = replaceProbe(
      probes,
      probe("reachability", "failed", message, {
        latencyMs: Date.now() - started,
      }),
    );
    return probes;
  }
}

function parseSse(text: string): SseParseResult {
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
    const parsed = parseJson(data);
    if (parsed !== undefined) events.push(parsed);
  }
  return { events, done };
}

function eventType(event: unknown): string | undefined {
  if (!event || typeof event !== "object") return undefined;
  const type = (event as { type?: unknown }).type;
  return typeof type === "string" ? type : undefined;
}

function hasFieldValue(
  value: unknown,
  key: string,
  expected: string,
  depth = 0,
): boolean {
  if (depth > 7 || !value || typeof value !== "object") return false;
  if (Array.isArray(value)) {
    return value.some((item) => hasFieldValue(item, key, expected, depth + 1));
  }
  const record = value as Record<string, unknown>;
  if (record[key] === expected) return true;
  return Object.values(record).some((item) =>
    hasFieldValue(item, key, expected, depth + 1),
  );
}

function collectToolEvidence(
  value: unknown,
  names: string[],
  argumentsList: string[],
  depth = 0,
) {
  if (depth > 7 || !value || typeof value !== "object") return;
  if (Array.isArray(value)) {
    value.forEach((item) =>
      collectToolEvidence(item, names, argumentsList, depth + 1),
    );
    return;
  }
  const record = value as Record<string, unknown>;
  if (record.type === "function_call" || record.type === "tool_use") {
    if (typeof record.name === "string") names.push(record.name);
    if (
      typeof record.arguments === "string" &&
      record.arguments.length > 0
    ) {
      argumentsList.push(record.arguments);
    } else if (
      record.input &&
      typeof record.input === "object" &&
      !Array.isArray(record.input) &&
      Object.keys(record.input).length > 0
    ) {
      argumentsList.push(JSON.stringify(record.input));
    }
  }
  if (record.function && typeof record.function === "object") {
    const fn = record.function as Record<string, unknown>;
    if (typeof fn.name === "string") names.push(fn.name);
    if (typeof fn.arguments === "string" && fn.arguments.length > 0) {
      argumentsList.push(fn.arguments);
    }
  }
  if (
    typeof record.delta === "string" &&
    typeof record.type === "string" &&
    record.type.includes("function_call_arguments")
  ) {
    argumentsList.push(record.delta);
  }
  if (
    typeof record.partial_json === "string" &&
    record.type === "input_json_delta"
  ) {
    argumentsList.push(record.partial_json);
  }
  Object.values(record).forEach((item) =>
    collectToolEvidence(item, names, argumentsList, depth + 1),
  );
}

function validProbeArguments(values: string[]): boolean {
  const valid = (value: string) => {
    const parsed = parseJson(value);
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

function streamCompatible(provider: ProviderId, parsed: SseParseResult) {
  const types = parsed.events.map(eventType);
  if (provider === "openai") {
    return (
      types.includes("response.output_text.delta") &&
      types.includes("response.completed")
    );
  }
  return (
    types.includes("message_start") &&
    parsed.events.some(
      (event) =>
        eventType(event) === "content_block_delta" &&
        hasFieldValue(event, "type", "text_delta"),
    ) &&
    types.includes("message_stop")
  );
}

function toolCompatible(provider: ProviderId, parsed: SseParseResult) {
  const names: string[] = [];
  const argumentsList: string[] = [];
  parsed.events.forEach((event) =>
    collectToolEvidence(event, names, argumentsList),
  );
  const terminal =
    provider === "openai"
      ? parsed.events.some(
          (event) => eventType(event) === "response.completed",
        )
      : parsed.events.some((event) => eventType(event) === "message_stop");
  return (
    terminal &&
    names.includes(PROBE_NAME) &&
    validProbeArguments(argumentsList)
  );
}

function textProbePayload(provider: ProviderId, model: string) {
  if (provider === "openai") {
    return {
      model,
      input: "Reply only with OK.",
      max_output_tokens: 16,
      store: false,
      stream: true,
    };
  }
  return {
    model,
    max_tokens: 8,
    messages: [{ role: "user", content: "Reply only with OK." }],
    stream: true,
  };
}

function toolProbePayload(provider: ProviderId, model: string) {
  const parameters = {
    type: "object",
    properties: {
      value: { type: "string", enum: ["ok"] },
    },
    required: ["value"],
    additionalProperties: false,
  };
  if (provider === "openai") {
    return {
      model,
      input:
        "Call kiln_capability_probe once with value ok. Do not answer otherwise.",
      max_output_tokens: 64,
      parallel_tool_calls: false,
      store: false,
      stream: true,
      tools: [
        {
          type: "function",
          name: PROBE_NAME,
          description:
            "Synthetic no-op used only to verify the provider protocol.",
          parameters,
          strict: true,
        },
      ],
      tool_choice: { type: "function", name: PROBE_NAME },
    };
  }
  return {
    model,
    max_tokens: 64,
    messages: [
      {
        role: "user",
        content:
          "Call kiln_capability_probe once with value ok. Do not answer otherwise.",
      },
    ],
    stream: true,
    tools: [
      {
        name: PROBE_NAME,
        description:
          "Synthetic no-op used only to verify the provider protocol.",
        input_schema: parameters,
      },
    ],
    tool_choice: {
      type: "tool",
      name: PROBE_NAME,
      disable_parallel_tool_use: true,
    },
  };
}

async function runInferenceProbe(
  provider: ProviderId,
  apiKey: string,
  model: string,
  kind: "streaming" | "toolCompatibility",
): Promise<ProbeResult> {
  const started = Date.now();
  const cloud = CLOUD[provider];
  try {
    const response = await fetchOnce(
      cloud.generationUrl,
      {
        method: "POST",
        headers: cloudHeaders(provider, apiKey, true),
        body: JSON.stringify(
          kind === "streaming"
            ? textProbePayload(provider, model)
            : toolProbePayload(provider, model),
        ),
      },
      INFERENCE_TIMEOUT_MS,
    );
    const latencyMs = Date.now() - started;

    if (isRedirect(response.status)) {
      await response.body?.cancel();
      return probe(
        kind,
        "failed",
        "Kiln refused an unexpected redirect before forwarding credentials.",
        { latencyMs },
      );
    }
    if (!response.ok) {
      await response.body?.cancel();
      return probe(
        kind,
        transientStatus(response.status) ? "inconclusive" : "failed",
        transientStatus(response.status)
          ? "The model probe could not be completed right now."
          : "The selected model did not accept this capability probe.",
        { latencyMs },
      );
    }

    const parsed = parseSse(await readBoundedText(response));
    const compatible =
      kind === "streaming"
        ? streamCompatible(provider, parsed)
        : toolCompatible(provider, parsed);
    return probe(
      kind,
      compatible ? "passed" : "failed",
      compatible
        ? kind === "streaming"
          ? "The selected model completed a compatible event stream."
          : "The selected model emitted a valid synthetic tool request. Nothing was executed."
        : kind === "streaming"
          ? "The selected model did not complete Kiln's streaming contract."
          : "The selected model did not emit Kiln's required tool-call shape.",
      { latencyMs },
    );
  } catch (cause) {
    return probe(
      kind,
      "inconclusive",
      cause instanceof BodyTooLarge
        ? "The model response exceeded the diagnostic safety limit."
        : isTimeoutError(cause)
          ? "The model probe timed out."
          : "The model probe could not reach the pinned provider origin.",
      { latencyMs: Date.now() - started },
    );
  }
}

async function runDiagnostics(
  provider: ProviderId,
  apiKey: string,
  model: string | undefined,
  verify: boolean,
) {
  let probes = await runDiscovery(provider, apiKey);
  if (!verify) return buildReport(provider, model, probes);

  const reachability = probes.find((item) => item.key === "reachability");
  const authentication = probes.find((item) => item.key === "authentication");
  if (
    reachability?.status !== "passed" ||
    authentication?.status !== "passed"
  ) {
    return buildReport(provider, model, probes);
  }
  if (!model?.trim()) {
    probes = replaceProbe(
      probes,
      probe("streaming", "not_run", "Choose a model before verification."),
    );
    probes = replaceProbe(
      probes,
      probe(
        "toolCompatibility",
        "not_run",
        "Choose a model before verification.",
      ),
    );
    return buildReport(provider, model, probes);
  }

  probes = replaceProbe(
    probes,
    await runInferenceProbe(provider, apiKey, model.trim(), "streaming"),
  );
  probes = replaceProbe(
    probes,
    await runInferenceProbe(
      provider,
      apiKey,
      model.trim(),
      "toolCompatibility",
    ),
  );
  return buildReport(provider, model, probes);
}

function extractOpenAIText(payload: unknown): string {
  if (!payload || typeof payload !== "object") return "";
  const record = payload as {
    output_text?: string;
    output?: Array<{
      content?: Array<{ type?: string; text?: string }>;
    }>;
  };

  if (record.output_text) return record.output_text;
  return (
    record.output
      ?.flatMap((item) => item.content ?? [])
      .filter((item) => item.type === "output_text" || item.type === "text")
      .map((item) => item.text ?? "")
      .join("") ?? ""
  );
}

function extractAnthropicText(payload: unknown): string {
  if (!payload || typeof payload !== "object") return "";
  const record = payload as {
    content?: Array<{ type?: string; text?: string }>;
  };
  return (
    record.content
      ?.filter((item) => item.type === "text")
      .map((item) => item.text ?? "")
      .join("") ?? ""
  );
}

export async function POST(request: NextRequest) {
  let body: ProviderRequest;
  try {
    const declaredLength = Number(request.headers.get("content-length"));
    if (Number.isFinite(declaredLength) && declaredLength > BODY_LIMIT) {
      return error("Request body exceeds the safety limit", 413);
    }
    const parsed = JSON.parse(
      await readBoundedStream(request.body),
    ) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return error("Request body must be a JSON object");
    }
    body = parsed as ProviderRequest;
  } catch (cause) {
    if (cause instanceof BodyTooLarge) {
      return error("Request body exceeds the safety limit", 413);
    }
    return error("Request body must be valid JSON");
  }

  if (body.provider !== "openai" && body.provider !== "anthropic") {
    return error("Unsupported cloud provider");
  }
  if (
    (body.apiKey !== undefined &&
      (typeof body.apiKey !== "string" ||
        body.apiKey.length > API_KEY_LIMIT)) ||
    (body.model !== undefined &&
      (typeof body.model !== "string" ||
        body.model.length > MODEL_LIMIT)) ||
    (body.messages !== undefined &&
      (!Array.isArray(body.messages) ||
        body.messages.length > MESSAGE_LIMIT ||
        body.messages.some(
          (message) =>
            !message ||
            typeof message !== "object" ||
            (message.role !== "user" && message.role !== "assistant") ||
            typeof message.content !== "string" ||
            message.content.length > MESSAGE_CONTENT_LIMIT,
        )))
  ) {
    return error("Provider request fields are invalid or exceed safety limits");
  }
  const provider = body.provider;
  const apiKey = body.apiKey?.trim();

  if (body.action === "test" || body.action === "verify") {
    if (!apiKey) {
      let probes = emptyProbes();
      probes = replaceProbe(
        probes,
        probe(
          "authentication",
          "failed",
          "Enter a session API key before testing this provider.",
        ),
      );
      return json({
        ok: true,
        report: buildReport(provider, body.model, probes),
      });
    }
    const report = await runDiagnostics(
      provider,
      apiKey,
      body.model,
      body.action === "verify",
    );
    return json({ ok: true, report });
  }

  if (body.action !== "chat") {
    return error("Unsupported provider action");
  }
  if (!apiKey) return error("A session API key is required");
  if (!body.model?.trim() || !body.messages?.length) {
    return error("A model and at least one message are required");
  }

  const cloud = CLOUD[provider];
  const payload =
    provider === "openai"
      ? {
          model: body.model.trim(),
          input: body.messages,
          store: false,
        }
      : {
          model: body.model.trim(),
          max_tokens: 4096,
          messages: body.messages,
        };

  try {
    const response = await fetchOnce(
      cloud.generationUrl,
      {
        method: "POST",
        headers: cloudHeaders(provider, apiKey),
        body: JSON.stringify(payload),
      },
      INFERENCE_TIMEOUT_MS,
    );
    if (isRedirect(response.status)) {
      await response.body?.cancel();
      return error("The provider returned an unexpected redirect", 502);
    }
    const text = await readBoundedText(response);
    if (!response.ok) {
      return error(
        response.status === 401 || response.status === 403
          ? "The provider did not authorize this request"
          : "The provider could not complete this request",
        response.status >= 400 && response.status < 600
          ? response.status
          : 502,
      );
    }
    const parsed = parseJson(text);
    if (parsed === undefined) {
      return error("The provider returned an unreadable response", 502);
    }
    return json({
      ok: true,
      text:
        provider === "openai"
          ? extractOpenAIText(parsed)
          : extractAnthropicText(parsed),
    });
  } catch (cause) {
    return error(
      cause instanceof BodyTooLarge
        ? "The provider response exceeded the safety limit"
        : isTimeoutError(cause)
          ? "The provider request timed out"
          : "The pinned provider origin could not be reached",
      502,
    );
  }
}
