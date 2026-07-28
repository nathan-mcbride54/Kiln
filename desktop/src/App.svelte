<script lang="ts">
  import { onMount } from "svelte";
  import {
    isDesktopRuntime,
    listProviderCapabilities,
    sendChat,
    testProviderConnection,
  } from "./lib/bridge";
  import type {
    ChatMessage,
    ProviderCapabilities,
    ProviderConfig,
    ProviderId,
  } from "./lib/types";

  type View = "workbench" | "connections" | "roadmap";
  type InspectorTab = "changes" | "activity";
  type TaskState = "working" | "review" | "queued" | "done";

  interface Task {
    id: string;
    title: string;
    workspace: string;
    branch: string;
    state: TaskState;
    additions: number;
    deletions: number;
    updated: string;
  }

  interface UiMessage extends ChatMessage {
    id: string;
    label: string;
    time: string;
    model?: string;
    note?: string;
  }

  interface RoadmapPhase {
    id: string;
    title: string;
    horizon: string;
    status: "now" | "next" | "later";
    summary: string;
    outcomes: string[];
  }

  const tasks: Task[] = [
    {
      id: "provider-router",
      title: "Build the provider router",
      workspace: "kiln",
      branch: "feat/provider-router",
      state: "working",
      additions: 284,
      deletions: 31,
      updated: "now",
    },
    {
      id: "trust-controls",
      title: "Design trust controls",
      workspace: "kiln",
      branch: "feat/trust-controls",
      state: "review",
      additions: 112,
      deletions: 18,
      updated: "8m",
    },
    {
      id: "acp-bridge",
      title: "Prototype the ACP bridge",
      workspace: "kiln",
      branch: "spike/acp",
      state: "queued",
      additions: 0,
      deletions: 0,
      updated: "24m",
    },
    {
      id: "shell-layout",
      title: "Polish desktop shell",
      workspace: "kiln",
      branch: "feat/shell",
      state: "done",
      additions: 396,
      deletions: 72,
      updated: "1h",
    },
  ];

  let providers: ProviderConfig[] = [
    {
      id: "openai",
      name: "OpenAI",
      shortName: "OA",
      protocol: "Responses API",
      description: "First-class reasoning and tool-use through OpenAI Responses.",
      baseUrl: "https://api.openai.com/v1",
      model: "gpt-5.6-terra",
      apiKey: "",
      apiKeyRequired: true,
      state: "untested",
      accent: "#a8e9cb",
    },
    {
      id: "anthropic",
      name: "Anthropic",
      shortName: "AN",
      protocol: "Messages API",
      description: "Claude models through the native Messages interface.",
      baseUrl: "https://api.anthropic.com/v1",
      model: "claude-sonnet-4-8",
      apiKey: "",
      apiKeyRequired: true,
      state: "untested",
      accent: "#e8a889",
    },
    {
      id: "local",
      name: "Local server",
      shortName: "LL",
      protocol: "OpenAI-compatible",
      description: "Ollama, llama.cpp, vLLM, or your own compatible endpoint.",
      baseUrl: "http://127.0.0.1:11434/v1",
      model: "qwen3-coder",
      apiKey: "",
      apiKeyRequired: false,
      state: "untested",
      accent: "#d3d98a",
    },
  ];

  const initialMessages: UiMessage[] = [
    {
      id: "m1",
      role: "user",
      label: "You",
      time: "14:06",
      content:
        "Create the provider boundary first. Keep credentials ephemeral, support native OpenAI and Anthropic APIs, and let me point the local adapter at any compatible server.",
    },
    {
      id: "m2",
      role: "assistant",
      label: "Kiln",
      time: "14:07",
      model: "gpt-5.6-terra",
      note: "Planned 4 steps · inspected 9 files",
      content:
        "I found a clean seam between the workbench and the runtime. I’m implementing one typed request shape with three protocol adapters, then exposing only three desktop commands: discover capabilities, test a connection, and send a chat request.\n\nCredentials stay inside the current request and are redacted from Rust diagnostics. The local adapter accepts a custom base URL; cloud providers remain pinned to their official origins.",
    },
    {
      id: "m3",
      role: "assistant",
      label: "Kiln",
      time: "14:09",
      model: "activity",
      note: "Awaiting review",
      content:
        "Provider boundary is in place. OpenAI uses Responses, Anthropic uses Messages, and local servers use Chat Completions. I also added structured errors so the interface can distinguish authentication, rate limits, timeouts, and unreachable local servers.",
    },
  ];

  const roadmap: RoadmapPhase[] = [
    {
      id: "H0",
      title: "Beautiful walking skeleton",
      horizon: "Weeks 0–2",
      status: "now",
      summary: "A dependable desktop loop with real provider calls and visible agency.",
      outcomes: [
        "Windows and Linux development builds",
        "OpenAI, Anthropic, and local adapters",
        "Conversation, activity, and diff surfaces",
      ],
    },
    {
      id: "H1",
      title: "Safe local execution",
      horizon: "Weeks 2–5",
      status: "next",
      summary: "Workspace-aware tools with explicit trust and approval boundaries.",
      outcomes: [
        "Filesystem and command tools",
        "Ask, allow once, and session policies",
        "Cancellation and durable run history",
      ],
    },
    {
      id: "H2",
      title: "Agent interoperability",
      horizon: "Weeks 5–8",
      status: "next",
      summary: "Treat external agents and tool ecosystems as first-class citizens.",
      outcomes: [
        "ACP client bridge",
        "MCP server management",
        "Portable session transcripts",
      ],
    },
    {
      id: "H3",
      title: "Parallel work",
      horizon: "Weeks 8–12",
      status: "later",
      summary: "Run focused agents in isolated worktrees without losing the plot.",
      outcomes: [
        "Task graph and subagents",
        "Git worktree isolation",
        "Unified review and merge flow",
      ],
    },
    {
      id: "H4",
      title: "Platform hardening",
      horizon: "After v1",
      status: "later",
      summary: "Polished installers, updates, accessibility, and macOS readiness.",
      outcomes: [
        "Signed Windows and Linux releases",
        "Crash recovery and diagnostics",
        "macOS packaging and notarization",
      ],
    },
  ];

  const files = [
    { path: "src/providers/openai.rs", additions: 86, deletions: 3 },
    { path: "src/providers/anthropic.rs", additions: 79, deletions: 5 },
    { path: "src/providers/local.rs", additions: 71, deletions: 8 },
    { path: "src/commands.rs", additions: 48, deletions: 15 },
  ];

  const activity = [
    {
      icon: "↳",
      title: "Read provider contracts",
      detail: "9 files · 1,842 lines",
      time: "14:07",
      tone: "quiet",
    },
    {
      icon: "✓",
      title: "Added typed provider boundary",
      detail: "Three adapters · one command surface",
      time: "14:08",
      tone: "good",
    },
    {
      icon: "⌁",
      title: "Ran Rust checks",
      detail: "18 tests passed · 0 warnings",
      time: "14:09",
      tone: "good",
    },
    {
      icon: "◇",
      title: "Ready for review",
      detail: "284 additions · 31 deletions",
      time: "now",
      tone: "accent",
    },
  ];

  let activeView: View = "workbench";
  let activeTaskId = tasks[0].id;
  let activeProviderId: ProviderId = "openai";
  let inspectorTab: InspectorTab = "changes";
  let messages: UiMessage[] = initialMessages;
  let draft = "";
  let running = false;
  let capabilities: ProviderCapabilities[] = [];
  let toast = "";
  let mobileSidebarOpen = false;

  $: activeTask = tasks.find((task) => task.id === activeTaskId) ?? tasks[0];
  $: activeProvider =
    providers.find((provider) => provider.id === activeProviderId) ?? providers[0];
  $: connectedCount = providers.filter((provider) => provider.state === "ready").length;

  onMount(async () => {
    try {
      capabilities = await listProviderCapabilities();
    } catch {
      capabilities = [];
    }
  });

  function patchProvider(
    id: ProviderId,
    patch: Partial<ProviderConfig>,
  ): void {
    providers = providers.map((provider) =>
      provider.id === id ? { ...provider, ...patch } : provider,
    );
  }

  async function testConnection(id: ProviderId): Promise<void> {
    const provider = providers.find((item) => item.id === id);
    if (!provider) return;

    if (provider.apiKeyRequired && !provider.apiKey.trim()) {
      patchProvider(id, {
        state: "error",
        message: "Add a key for this session before testing.",
      });
      return;
    }

    patchProvider(id, { state: "testing", message: "Testing the route…" });

    try {
      const response = await testProviderConnection(provider);
      patchProvider(id, {
        state: response.connected ? "ready" : "error",
        latency: response.latencyMs,
        message: response.message,
      });
      toast = response.connected
        ? `${provider.name} is ready`
        : `${provider.name} did not connect`;
    } catch (error) {
      patchProvider(id, {
        state: "error",
        message: error instanceof Error ? error.message : "Connection failed.",
      });
      toast = `Couldn’t reach ${provider.name}`;
    }

    window.setTimeout(() => (toast = ""), 3200);
  }

  async function submitPrompt(): Promise<void> {
    const content = draft.trim();
    if (!content || running) return;

    const userMessage: UiMessage = {
      id: crypto.randomUUID(),
      role: "user",
      label: "You",
      time: new Date().toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
      }),
      content,
    };

    const messagesBeforeSend = messages;
    messages = [...messagesBeforeSend, userMessage];
    draft = "";
    running = true;

    const requestMessages: ChatMessage[] = [...messagesBeforeSend, userMessage]
      .filter((message) => message.model !== "activity")
      .map(({ role, content: messageContent }) => ({
        role,
        content: messageContent,
      }));

    try {
      const response = await sendChat({
        provider: activeProvider.id,
        credentials: {
          apiKey: activeProvider.apiKey || undefined,
        },
        baseUrl: activeProvider.baseUrl || undefined,
        model: activeProvider.model,
        messages: requestMessages,
        maxOutputTokens: 4096,
      });

      messages = [
        ...messages,
        {
          id: response.id ?? crypto.randomUUID(),
          role: "assistant",
          label: "Kiln",
          time: new Date().toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
          }),
          model: response.model,
          note: response.usage.totalTokens
            ? `${response.usage.totalTokens.toLocaleString()} tokens`
            : "Completed",
          content: response.content,
        },
      ];
    } catch (error) {
      messages = [
        ...messages,
        {
          id: crypto.randomUUID(),
          role: "assistant",
          label: "Kiln",
          time: "now",
          model: "connection error",
          note: "Your draft is preserved above",
          content:
            error instanceof Error
              ? error.message
              : "The selected provider could not complete this request.",
        },
      ];
    } finally {
      running = false;
    }
  }

  function handleComposerKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void submitPrompt();
    }
  }

  function selectView(view: View): void {
    activeView = view;
    mobileSidebarOpen = false;
  }

  function stateLabel(state: ProviderConfig["state"]): string {
    if (state === "ready") return "Ready";
    if (state === "testing") return "Testing";
    if (state === "error") return "Needs attention";
    return "Not tested";
  }
</script>

<svelte:head>
  <title>Kiln — Local-first agent workbench</title>
  <meta
    name="description"
    content="One calm surface for every coding agent."
  />
</svelte:head>

<div class="app-shell" class:sidebar-open={mobileSidebarOpen}>
  <header class="titlebar">
    <div class="titlebar-brand">
      <button
        class="mobile-menu icon-button"
        type="button"
        aria-label="Toggle navigation"
        aria-expanded={mobileSidebarOpen}
        onclick={() => (mobileSidebarOpen = !mobileSidebarOpen)}
      >
        <span></span><span></span><span></span>
      </button>
      <div class="kiln-mark" aria-hidden="true">
        <span></span>
      </div>
      <span class="wordmark">Kiln</span>
      <span class="build-tag">alpha</span>
    </div>

    <div class="titlebar-center" data-tauri-drag-region>
      <span class="workspace-dot"></span>
      <span>kiln</span>
      <span class="slash">/</span>
      <span>{activeTask.branch}</span>
    </div>

    <div class="window-actions">
      <span class:desktop-live={isDesktopRuntime()} class="runtime-state">
        {isDesktopRuntime() ? "Desktop runtime" : "Browser preview"}
      </span>
      <button class="icon-button" type="button" aria-label="Open command palette">
        <span class="command-glyph">⌘</span>
      </button>
    </div>
  </header>

  <aside class="sidebar" aria-label="Primary navigation">
    <div class="sidebar-top">
      <button class="new-task" type="button" onclick={() => (toast = "New task is on the H1 roadmap")}>
        <span class="plus">+</span>
        <span>New task</span>
        <kbd>⌘N</kbd>
      </button>

      <nav class="primary-nav">
        <button
          type="button"
          class:active={activeView === "workbench"}
          aria-current={activeView === "workbench" ? "page" : undefined}
          onclick={() => selectView("workbench")}
        >
          <span class="nav-icon">⌁</span>
          <span>Workbench</span>
          <span class="nav-count">{tasks.length}</span>
        </button>
        <button
          type="button"
          class:active={activeView === "connections"}
          aria-current={activeView === "connections" ? "page" : undefined}
          onclick={() => selectView("connections")}
        >
          <span class="nav-icon">◉</span>
          <span>Connections</span>
          <span class="connection-lights" aria-label={`${connectedCount} connected`}>
            {#each providers as provider}
              <i
                class:ready={provider.state === "ready"}
                style={`--provider: ${provider.accent}`}
              ></i>
            {/each}
          </span>
        </button>
        <button
          type="button"
          class:active={activeView === "roadmap"}
          aria-current={activeView === "roadmap" ? "page" : undefined}
          onclick={() => selectView("roadmap")}
        >
          <span class="nav-icon">◇</span>
          <span>Roadmap</span>
          <span class="nav-count">H0</span>
        </button>
      </nav>
    </div>

    <div class="task-section">
      <div class="section-label">
        <span>Tasks</span>
        <button type="button" aria-label="Task options">•••</button>
      </div>

      <div class="task-list">
        {#each tasks as task}
          <button
            type="button"
            class="task-row"
            class:active={task.id === activeTaskId && activeView === "workbench"}
            onclick={() => {
              activeTaskId = task.id;
              selectView("workbench");
            }}
          >
            <span class:working={task.state === "working"} class={`task-state ${task.state}`}>
              {task.state === "working"
                ? "●"
                : task.state === "review"
                  ? "◆"
                  : task.state === "done"
                    ? "✓"
                    : "○"}
            </span>
            <span class="task-copy">
              <strong>{task.title}</strong>
              <small>
                {task.branch}
                {#if task.additions}
                  <em class="positive">+{task.additions}</em>
                  <em class="negative">−{task.deletions}</em>
                {/if}
              </small>
            </span>
            <time>{task.updated}</time>
          </button>
        {/each}
      </div>
    </div>

    <div class="sidebar-footer">
      <div class="platform-row">
        <span class="platform-badge">W</span>
        <span class="platform-badge">L</span>
        <span class="platform-badge future">M</span>
        <span class="platform-copy">
          <strong>Desktop first</strong>
          <small>Windows · Linux · macOS later</small>
        </span>
      </div>
      <button class="settings-row" type="button" onclick={() => selectView("connections")}>
        <span>⚙</span>
        <span>Settings</span>
        <kbd>⌘,</kbd>
      </button>
    </div>
  </aside>

  <main class="main-surface">
    {#if activeView === "workbench"}
      <section class="workbench">
        <header class="workspace-header">
          <div class="workspace-heading">
            <div class="eyebrow">
              <span class={`task-state ${activeTask.state}`}></span>
              {activeTask.state === "working"
                ? "Agent working"
                : activeTask.state === "review"
                  ? "Ready for review"
                  : activeTask.state === "done"
                    ? "Completed"
                    : "Queued"}
            </div>
            <h1>{activeTask.title}</h1>
          </div>

          <div class="workspace-actions">
            <label class="provider-picker">
              <span
                class="provider-swatch"
                style={`--provider: ${activeProvider.accent}`}
              ></span>
              <select bind:value={activeProviderId} aria-label="Active provider">
                {#each providers as provider}
                  <option value={provider.id}>{provider.name} · {provider.model}</option>
                {/each}
              </select>
              <span class="chevron">⌄</span>
            </label>
            <button class="secondary-button" type="button">
              <span>↗</span> Open workspace
            </button>
            <button class="more-button" type="button" aria-label="More task actions">•••</button>
          </div>
        </header>

        <div class="workbench-grid">
          <section class="conversation-pane" aria-label="Conversation">
            <div class="conversation-scroll">
              <div class="conversation-intro">
                <div class="branch-pill">
                  <span>⌁</span>
                  {activeTask.branch}
                </div>
                <span>Local workspace · started today at 14:06</span>
              </div>

              {#each messages as message}
                <article
                  class="message"
                  class:user-message={message.role === "user"}
                  class:activity-message={message.model === "activity"}
                >
                  <div class="message-identity">
                    <div
                      class:agent-avatar={message.role === "assistant"}
                      class:user-avatar={message.role === "user"}
                      class="avatar"
                    >
                      {message.role === "assistant" ? "K" : "N"}
                    </div>
                    <div>
                      <strong>{message.label}</strong>
                      <span>{message.time}</span>
                    </div>
                    {#if message.model}
                      <span class="model-chip">{message.model}</span>
                    {/if}
                  </div>
                  <div class="message-body">
                    {message.content}
                  </div>
                  {#if message.note}
                    <div class="message-note">
                      {#if message.model === "activity"}
                        <span class="pulse-ring"></span>
                      {:else}
                        <span>✓</span>
                      {/if}
                      {message.note}
                    </div>
                  {/if}
                </article>
              {/each}

              {#if running}
                <article class="message thinking-message" aria-live="polite">
                  <div class="message-identity">
                    <div class="avatar agent-avatar">K</div>
                    <div>
                      <strong>Kiln</strong>
                      <span>now</span>
                    </div>
                  </div>
                  <div class="thinking-line">
                    <i></i><i></i><i></i>
                    <span>Working through the provider boundary…</span>
                  </div>
                </article>
              {/if}
            </div>

            <form
              class="composer-wrap"
              onsubmit={(event) => {
                event.preventDefault();
                void submitPrompt();
              }}
            >
              {#if activeProvider.state !== "ready"}
                <button
                  class="connection-hint"
                  type="button"
                  onclick={() => selectView("connections")}
                >
                  <span
                    class="provider-swatch"
                    style={`--provider: ${activeProvider.accent}`}
                  ></span>
                  {activeProvider.name} is not connected
                  <strong>Set up →</strong>
                </button>
              {/if}
              <div class="composer">
                <textarea
                  bind:value={draft}
                  onkeydown={handleComposerKeydown}
                  rows="3"
                  placeholder="Ask Kiln to inspect, change, explain, or plan…"
                  aria-label="Message Kiln"
                ></textarea>
                <div class="composer-footer">
                  <div class="composer-tools">
                    <button type="button" aria-label="Attach context">＋</button>
                    <button type="button" aria-label="Reference file">@</button>
                    <span class="context-meter">
                      <i></i>
                      12.4k context
                    </span>
                  </div>
                  <div class="send-actions">
                    <span><kbd>⌘</kbd><kbd>↵</kbd></span>
                    <button
                      class="send-button"
                      type="submit"
                      disabled={!draft.trim() || running}
                      aria-label="Send message"
                    >
                      {running ? "■" : "↑"}
                    </button>
                  </div>
                </div>
              </div>
              <p class="composer-caption">
                Kiln can make mistakes. Review commands and changes before approval.
              </p>
            </form>
          </section>

          <aside class="inspector" aria-label="Task inspector">
            <div class="inspector-tabs" role="tablist">
              <button
                type="button"
                role="tab"
                aria-selected={inspectorTab === "changes"}
                class:active={inspectorTab === "changes"}
                onclick={() => (inspectorTab = "changes")}
              >
                Changes
                <span>{files.length}</span>
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={inspectorTab === "activity"}
                class:active={inspectorTab === "activity"}
                onclick={() => (inspectorTab = "activity")}
              >
                Activity
                <span>{activity.length}</span>
              </button>
            </div>

            {#if inspectorTab === "changes"}
              <div class="inspector-summary">
                <div>
                  <strong>{files.length} files changed</strong>
                  <span>Working tree</span>
                </div>
                <div class="diff-total">
                  <span>+{activeTask.additions}</span>
                  <span>−{activeTask.deletions}</span>
                </div>
              </div>

              <div class="file-list">
                {#each files as file, index}
                  <button type="button" class:active={index === 0}>
                    <span class="file-icon">R</span>
                    <span class="file-copy">
                      <strong>{file.path.split("/").at(-1)}</strong>
                      <small>{file.path.split("/").slice(0, -1).join("/")}</small>
                    </span>
                    <span class="file-numbers">
                      <em>+{file.additions}</em>
                      <em>−{file.deletions}</em>
                    </span>
                  </button>
                {/each}
              </div>

              <div class="diff-card">
                <div class="diff-head">
                  <div>
                    <span class="file-icon">R</span>
                    <strong>openai.rs</strong>
                  </div>
                  <span>•••</span>
                </div>
                <div class="diff-code" aria-label="Code change preview">
                  <div class="code-line context">
                    <span class="line-number">41</span>
                    <code>async fn send(&amp;self, request: &amp;ChatRequest)</code>
                  </div>
                  <div class="code-line removed">
                    <span class="line-number">42</span>
                    <code>- self.client.chat(request).await</code>
                  </div>
                  <div class="code-line added">
                    <span class="line-number">42</span>
                    <code>+ let body = ResponsesBody::from(request);</code>
                  </div>
                  <div class="code-line added">
                    <span class="line-number">43</span>
                    <code>+ self.post("/v1/responses", body).await</code>
                  </div>
                  <div class="code-line context">
                    <span class="line-number">44</span>
                    <code>}</code>
                  </div>
                </div>
              </div>

              <div class="review-actions">
                <button class="secondary-button" type="button">Discard</button>
                <button class="primary-button" type="button">
                  Review changes
                  <span>→</span>
                </button>
              </div>
            {:else}
              <div class="activity-list">
                {#each activity as item, index}
                  <div class="activity-row">
                    <div class={`activity-icon ${item.tone}`}>{item.icon}</div>
                    {#if index < activity.length - 1}
                      <div class="activity-rail"></div>
                    {/if}
                    <div class="activity-copy">
                      <strong>{item.title}</strong>
                      <span>{item.detail}</span>
                    </div>
                    <time>{item.time}</time>
                  </div>
                {/each}
              </div>

              <div class="trust-card">
                <div class="trust-icon">⌁</div>
                <div>
                  <strong>Visible agency</strong>
                  <p>Every tool call, approval, and file change lands here as an inspectable event.</p>
                </div>
              </div>
            {/if}
          </aside>
        </div>
      </section>
    {:else if activeView === "connections"}
      <section class="page connections-page">
        <header class="page-header">
          <div>
            <div class="eyebrow">Provider control plane</div>
            <h1>Bring the models you trust.</h1>
            <p>
              Cloud-native protocols when available, one open route for your own server.
              Secrets live only in this running session.
            </p>
          </div>
          <div class="security-pill">
            <span>⌁</span>
            <div>
              <strong>Session-only credentials</strong>
              <small>Never written to project files or browser storage</small>
            </div>
          </div>
        </header>

        <div class="provider-grid">
          {#each providers as provider}
            <form
              class="provider-card"
              class:ready={provider.state === "ready"}
              class:error={provider.state === "error"}
              style={`--provider: ${provider.accent}`}
              onsubmit={(event) => {
                event.preventDefault();
                void testConnection(provider.id);
              }}
            >
              <div class="provider-card-head">
                <div class="provider-identity">
                  <div class="provider-monogram">{provider.shortName}</div>
                  <div>
                    <h2>{provider.name}</h2>
                    <span>{provider.protocol}</span>
                  </div>
                </div>
                <div class={`provider-status ${provider.state}`}>
                  <i></i>
                  {stateLabel(provider.state)}
                </div>
              </div>

              <p>{provider.description}</p>

              <div class="form-fields">
                <label>
                  <span>
                    API key
                    {#if !provider.apiKeyRequired}<small>optional</small>{/if}
                  </span>
                  <div class="secret-input">
                    <input
                      type="password"
                      autocomplete="off"
                      spellcheck="false"
                      value={provider.apiKey}
                      placeholder={provider.id === "openai"
                        ? "sk-proj-…"
                        : provider.id === "anthropic"
                          ? "sk-ant-…"
                          : "Optional bearer token"}
                      oninput={(event) =>
                        patchProvider(provider.id, {
                          apiKey: event.currentTarget.value,
                          state: "untested",
                          message: undefined,
                        })}
                    />
                    <span>session</span>
                  </div>
                </label>

                <label>
                  <span>{provider.id === "local" ? "Base URL" : "API origin"}</span>
                  <input
                    class="mono-input"
                    type="url"
                    value={provider.baseUrl}
                    disabled={provider.id !== "local"}
                    oninput={(event) =>
                      patchProvider(provider.id, {
                        baseUrl: event.currentTarget.value,
                        state: "untested",
                      })}
                  />
                </label>

                <label>
                  <span>Default model</span>
                  <input
                    class="mono-input"
                    type="text"
                    value={provider.model}
                    spellcheck="false"
                    oninput={(event) =>
                      patchProvider(provider.id, {
                        model: event.currentTarget.value,
                      })}
                  />
                </label>
              </div>

              <div class="provider-card-footer">
                <div class="connection-result" aria-live="polite">
                  {#if provider.message}
                    <span>{provider.message}</span>
                  {:else}
                    <span>
                      {provider.id === "local"
                        ? "Key is optional for local routes"
                        : "Key is held in memory only"}
                    </span>
                  {/if}
                  {#if provider.latency}
                    <strong>{provider.latency} ms</strong>
                  {/if}
                </div>
                <button
                  class="test-button"
                  type="submit"
                  disabled={provider.state === "testing"}
                >
                  {provider.state === "testing" ? "Testing…" : "Test connection"}
                  <span>→</span>
                </button>
              </div>
            </form>
          {/each}
        </div>

        <section class="capabilities-card">
          <div class="capabilities-heading">
            <div>
              <span class="eyebrow">Shared contract</span>
              <h2>One workbench, honest capabilities.</h2>
            </div>
            <p>
              Kiln normalizes the common path and keeps provider-specific behavior visible.
            </p>
          </div>
          <div class="capabilities-table">
            <div class="cap-row cap-header">
              <span>Capability</span>
              {#each providers as provider}
                <span>{provider.name}</span>
              {/each}
            </div>
            {#each [
              ["Native protocol", true, true, true],
              ["Custom endpoint", false, false, true],
              ["Model discovery", true, true, true],
              ["System messages", true, true, true],
              ["Streaming roadmap", true, true, true],
            ] as row}
              <div class="cap-row">
                <span>{row[0]}</span>
                {#each row.slice(1) as supported}
                  <span class:supported={supported === true}>
                    {supported === true ? "✓" : "—"}
                  </span>
                {/each}
              </div>
            {/each}
          </div>
          {#if capabilities.length}
            <div class="contract-note">
              <span>✓</span>
              Runtime reported {capabilities.length} provider contracts.
            </div>
          {/if}
        </section>
      </section>
    {:else}
      <section class="page roadmap-page">
        <header class="page-header roadmap-header">
          <div>
            <div class="eyebrow">Living product roadmap · 2026-07-28</div>
            <h1>Earn trust before adding autonomy.</h1>
            <p>
              Kiln grows from a sharp local workbench into an interoperable agent
              control plane. Each horizon ships only when its trust gate is real.
            </p>
          </div>
          <div class="roadmap-legend" aria-label="Roadmap status legend">
            <span><i class="now"></i> Now</span>
            <span><i class="next"></i> Next</span>
            <span><i class="later"></i> Later</span>
          </div>
        </header>

        <div class="principle-strip">
          <div>
            <span>01</span>
            <strong>Local by default</strong>
            <p>Your code, sessions, and policy stay on your machine.</p>
          </div>
          <div>
            <span>02</span>
            <strong>Visible agency</strong>
            <p>Plans, calls, changes, and approvals remain inspectable.</p>
          </div>
          <div>
            <span>03</span>
            <strong>Open seams</strong>
            <p>Providers, tools, and agents meet typed protocol boundaries.</p>
          </div>
          <div>
            <span>04</span>
            <strong>Fast recovery</strong>
            <p>Every run can stop, resume, explain itself, or roll back.</p>
          </div>
        </div>

        <div class="roadmap-timeline">
          {#each roadmap as phase, index}
            <article class={`roadmap-phase ${phase.status}`}>
              <div class="phase-marker">
                <span>{phase.id}</span>
                {#if index < roadmap.length - 1}<i></i>{/if}
              </div>
              <div class="phase-card">
                <header>
                  <div>
                    <span class="phase-horizon">{phase.horizon}</span>
                    <h2>{phase.title}</h2>
                  </div>
                  <span class={`phase-status ${phase.status}`}>
                    {phase.status}
                  </span>
                </header>
                <p>{phase.summary}</p>
                <ul>
                  {#each phase.outcomes as outcome}
                    <li><span>✓</span>{outcome}</li>
                  {/each}
                </ul>
              </div>
            </article>
          {/each}
        </div>

        <section class="platform-card">
          <div class="platform-copy-large">
            <span class="eyebrow">Platform sequence</span>
            <h2>Native where it matters. Portable everywhere else.</h2>
            <p>
              Rust owns execution, policy, and provider I/O. Svelte owns the calm
              interaction surface. Tauri carries both across platforms.
            </p>
          </div>
          <div class="platform-lanes">
            <div class="platform-lane active">
              <span class="platform-badge large">W</span>
              <div><strong>Windows</strong><small>Primary launch target</small></div>
              <em>H0</em>
            </div>
            <div class="platform-lane active">
              <span class="platform-badge large">L</span>
              <div><strong>Linux</strong><small>Primary launch target</small></div>
              <em>H0</em>
            </div>
            <div class="platform-lane">
              <span class="platform-badge large future">M</span>
              <div><strong>macOS</strong><small>Design-compatible now</small></div>
              <em>H4</em>
            </div>
          </div>
        </section>
      </section>
    {/if}
  </main>

  {#if mobileSidebarOpen}
    <button
      class="sidebar-scrim"
      type="button"
      aria-label="Close navigation"
      onclick={() => (mobileSidebarOpen = false)}
    ></button>
  {/if}

  {#if toast}
    <div class="toast" role="status">
      <span>✓</span>
      {toast}
    </div>
  {/if}
</div>
