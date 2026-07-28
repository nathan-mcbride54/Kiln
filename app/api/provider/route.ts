import { NextRequest, NextResponse } from "next/server";

type ProviderId = "openai" | "anthropic";
type InputMessage = { role: "user" | "assistant"; content: string };

type ProviderRequest = {
  action?: "test" | "chat";
  provider?: ProviderId;
  model?: string;
  apiKey?: string;
  messages?: InputMessage[];
};

export const dynamic = "force-dynamic";

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
    body = (await request.json()) as ProviderRequest;
  } catch {
    return error("Request body must be valid JSON");
  }

  if (body.provider !== "openai" && body.provider !== "anthropic") {
    return error("Unsupported cloud provider");
  }
  if (!body.apiKey?.trim()) {
    return error("An API key is required");
  }

  const isTest = body.action === "test";
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };

  try {
    if (body.provider === "openai") {
      headers.Authorization = `Bearer ${body.apiKey}`;

      if (isTest) {
        const response = await fetch("https://api.openai.com/v1/models", {
          headers,
          cache: "no-store",
        });
        if (!response.ok) {
          return error(`OpenAI returned ${response.status}`, response.status);
        }
        return json({ ok: true });
      }

      if (!body.model || !body.messages?.length) {
        return error("A model and at least one message are required");
      }

      const response = await fetch("https://api.openai.com/v1/responses", {
        method: "POST",
        headers,
        cache: "no-store",
        body: JSON.stringify({
          model: body.model,
          input: body.messages,
          store: false,
        }),
      });
      const payload = (await response.json()) as unknown;
      if (!response.ok) {
        return error(`OpenAI returned ${response.status}`, response.status);
      }
      return json({ ok: true, text: extractOpenAIText(payload) });
    }

    headers["x-api-key"] = body.apiKey;
    headers["anthropic-version"] = "2023-06-01";

    if (isTest) {
      const response = await fetch("https://api.anthropic.com/v1/models", {
        headers,
        cache: "no-store",
      });
      if (!response.ok) {
        return error(`Anthropic returned ${response.status}`, response.status);
      }
      return json({ ok: true });
    }

    if (!body.model || !body.messages?.length) {
      return error("A model and at least one message are required");
    }

    const response = await fetch("https://api.anthropic.com/v1/messages", {
      method: "POST",
      headers,
      cache: "no-store",
      body: JSON.stringify({
        model: body.model,
        max_tokens: 4096,
        messages: body.messages,
      }),
    });
    const payload = (await response.json()) as unknown;
    if (!response.ok) {
      return error(`Anthropic returned ${response.status}`, response.status);
    }
    return json({
      ok: true,
      text: extractAnthropicText(payload),
    });
  } catch {
    return error("The provider could not be reached", 502);
  }
}
