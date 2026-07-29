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
  roadmapLastReviewed,
  roadmapRevision,
} from "./roadmap.generated";

type ProviderId = "openai" | "anthropic" | "local";
type ViewId = "workbench" | "providers" | "roadmap";
type TaskStatus = "running" | "review" | "done" | "paused";

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
  status: "idle" | "testing" | "connected" | "error";
  statusText: string;
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
    endpoint: "https://api.anthropic.com",
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

  async function testProvider(id: ProviderId) {
    const config = configs[id];
    updateConfig(id, { status: "testing", statusText: "Testing connection…" });

    try {
      if (id === "local") {
        const endpoint = config.endpoint.replace(/\/$/, "");
        const response = await fetch(`${endpoint}/models`, {
          headers: config.apiKey
            ? { Authorization: `Bearer ${config.apiKey}` }
            : undefined,
        });
        if (!response.ok) {
          throw new Error(`Server returned ${response.status}`);
        }
      } else {
        if (!config.apiKey.trim()) {
          throw new Error("Add a session API key first");
        }
        const response = await fetch("/api/provider", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            action: "test",
            provider: id,
            apiKey: config.apiKey,
          }),
        });
        const result = (await response.json()) as {
          ok?: boolean;
          error?: string;
        };
        if (!response.ok || !result.ok) {
          throw new Error(result.error || "Connection failed");
        }
      }
      updateConfig(id, {
        status: "connected",
        statusText: "Connected",
      });
      setToast(`${providerMeta[id].name} is ready`);
    } catch (error) {
      updateConfig(id, {
        status: "error",
        statusText:
          error instanceof Error ? error.message : "Connection failed",
      });
    }
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
        const endpoint = config.endpoint.replace(/\/$/, "");
        const response = await fetch(`${endpoint}/chat/completions`, {
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
        });
        if (!response.ok) {
          throw new Error(`Local server returned ${response.status}`);
        }
        const result = (await response.json()) as {
          choices?: Array<{ message?: { content?: string } }>;
        };
        assistantText =
          result.choices?.[0]?.message?.content ??
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
                        : config.status === "connected"
                          ? "Connected"
                          : config.status === "error"
                            ? "Needs attention"
                            : "Not connected"}
                    </span>
                  </div>

                  <label>
                    <span>Model</span>
                    <input
                      value={config.model}
                      onChange={(event) =>
                        updateConfig(id, { model: event.target.value })
                      }
                      spellCheck={false}
                    />
                  </label>
                  <label>
                    <span>Endpoint</span>
                    <input
                      value={config.endpoint}
                      disabled={id !== "local"}
                      onChange={(event) =>
                        updateConfig(id, { endpoint: event.target.value })
                      }
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
                      onChange={(event) =>
                        updateConfig(id, {
                          apiKey: event.target.value,
                          status: "idle",
                          statusText:
                            id === "local"
                              ? "Ready to test"
                              : "Session key ready",
                        })
                      }
                      autoComplete="off"
                    />
                  </label>
                  <div className="provider-card-footer">
                    <div>
                      <span
                        className={`connection-light connection-light-${config.status}`}
                      />
                      <span>{config.statusText}</span>
                    </div>
                    <button
                      onClick={() => void testProvider(id)}
                      disabled={config.status === "testing"}
                    >
                      {config.status === "connected" ? "Retest" : "Test connection"}
                    </button>
                  </div>
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
              <span className="matrix-note">Advertised at connection time</span>
            </div>
            <div className="matrix-table" role="table" aria-label="Provider capabilities">
              <div className="matrix-row matrix-head" role="row">
                <span>Capability</span>
                <span>OpenAI</span>
                <span>Anthropic</span>
                <span>Local</span>
              </div>
              {[
                ["Streaming", "yes", "yes", "detect"],
                ["Tool calls", "yes", "yes", "detect"],
                ["Structured output", "yes", "partial", "detect"],
                ["Usage reporting", "yes", "yes", "detect"],
                ["Data destination", "remote", "remote", "device"],
              ].map((row) => (
                <div className="matrix-row" role="row" key={row[0]}>
                  <strong>{row[0]}</strong>
                  {row.slice(1).map((value, index) => (
                    <span className={`matrix-value matrix-${value}`} key={`${value}-${index}`}>
                      {value === "yes" ? "● Supported" : value}
                    </span>
                  ))}
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
              <strong>H0</strong>
              <small>Product foundation</small>
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
