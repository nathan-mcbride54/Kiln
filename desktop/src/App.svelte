<script lang="ts">
  import { onMount } from "svelte";
  import {
    cancelTurn,
    executeVisibleRepositoryTool,
    executeTurnStreaming,
    isDesktopRuntime,
    listRememberedProjects,
    listProviderCapabilities,
    loadApplicationEvents,
    openRepository,
    persistApplicationEvents,
    testProviderConnection,
  } from "./lib/bridge";
  import {
    type ApplicationEvent,
    type EventEnvelope,
    type EventMetadata,
  } from "./lib/events.ts";
  import { DurableTaskHistory } from "./lib/history.ts";
  import { initialSessionEvents } from "./lib/preview-session.ts";
  import { projectInspector } from "./lib/projector.ts";
  import {
    roadmap,
    roadmapLastReviewed,
    roadmapRevision,
  } from "./lib/roadmap.generated";
  import type {
    ChatMessage,
    ProjectDefaults,
    ProjectSnapshot,
    ProviderCapabilities,
    ProviderConfig,
    ProviderId,
    RememberedProject,
  } from "./lib/types";

  type View = "workbench" | "connections" | "roadmap";
  type InspectorTab = "changes" | "activity";
  type TaskState = "working" | "review" | "queued" | "done";
  const taskStreamId = "task:recorded-replay";

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

  const tasks: Task[] = [
    {
      id: "recorded-replay",
      title: "Polish the provider status card",
      workspace: "kiln",
      branch: "feat/provider-status",
      state: "review",
      additions: 18,
      deletions: 6,
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

  let files = [
    { path: "desktop/src/App.svelte", additions: 18, deletions: 6 },
  ];

  const taskHistory = new DurableTaskHistory(
    taskStreamId,
    "recorded-replay",
    isDesktopRuntime()
    ? []
      : initialSessionEvents,
  );
  let projection = taskHistory.projection;
  let activeView: View = "workbench";
  let activeTaskId = tasks[0].id;
  let activeProviderId: ProviderId = "openai";
  let inspectorTab: InspectorTab = "changes";
  let draft = "";
  let capabilities: ProviderCapabilities[] = [];
  let toast = "";
  let mobileSidebarOpen = false;
  let historyReady = !isDesktopRuntime();
  let submitting = false;
  let activeTurnId: string | undefined;
  let rememberedProjects: RememberedProject[] = [];
  let activeProject: ProjectSnapshot | undefined;
  let projectDialogOpen = false;
  let projectPath = "";
  let projectError = "";
  let openingProject = false;
  let inspectingWorkspace = false;
  let editingWorkspace = false;
  let editPath = "";
  let editContent = "";
  let editExpectedSha256: string | undefined;
  let editLoadedPath: string | undefined;
  let latestWriteDiff = "";

  $: messages = projection.messages;
  $: activity = projection.activity;
  $: inspector = projectInspector(projection);
  $: running = projection.running;
  $: activeTask = tasks.find((task) => task.id === activeTaskId) ?? tasks[0];
  $: changeAdditions = files.reduce((total, file) => total + file.additions, 0);
  $: changeDeletions = files.reduce((total, file) => total + file.deletions, 0);
  $: activeProvider =
    providers.find((provider) => provider.id === activeProviderId) ?? providers[0];
  $: connectedCount = providers.filter((provider) => provider.state === "ready").length;

  onMount(async () => {
    const [providerResult, historyResult, projectsResult] = await Promise.allSettled([
      listProviderCapabilities(),
      isDesktopRuntime()
        ? loadApplicationEvents(taskStreamId)
        : Promise.resolve(initialSessionEvents),
      listRememberedProjects(),
    ]);

    capabilities =
      providerResult.status === "fulfilled" ? providerResult.value : [];

    try {
      if (historyResult.status === "fulfilled") {
        replaceApplicationEvents(historyResult.value);
      } else {
        throw historyResult.reason;
      }
    } catch {
      toast = "Kiln couldn’t restore local task history.";
      window.setTimeout(() => (toast = ""), 4200);
    }
    if (projectsResult.status === "fulfilled") {
      rememberedProjects = projectsResult.value;
      const firstAvailable = rememberedProjects.find((project) => project.available);
      if (firstAvailable) selectProject(firstAvailable.project);
    } else {
      toast = "Kiln couldn’t refresh remembered repositories.";
      window.setTimeout(() => (toast = ""), 4200);
    }
    historyReady = true;
  });

  function replaceApplicationEvents(events: readonly EventEnvelope[]): void {
    projection = taskHistory.restore(events);
  }

  function patchProvider(
    id: ProviderId,
    patch: Partial<ProviderConfig>,
  ): void {
    providers = providers.map((provider) =>
      provider.id === id ? { ...provider, ...patch } : provider,
    );
  }

  function selectProject(project: ProjectSnapshot): void {
    activeProject = project;
    files = [];
    editPath = "";
    editContent = "";
    editExpectedSha256 = undefined;
    editLoadedPath = undefined;
    latestWriteDiff = "";
    const provider = project.defaults.provider;
    if (provider && providers.some((candidate) => candidate.id === provider)) {
      activeProviderId = provider;
      if (project.defaults.model) {
        patchProvider(provider, { model: project.defaults.model });
      }
    }
  }

  function showProjectDialog(project?: RememberedProject): void {
    projectPath = project?.project.root ?? activeProject?.root ?? "";
    projectError =
      project && !project.available
        ? project.unavailableReason ?? "This remembered repository is unavailable."
        : "";
    projectDialogOpen = true;
  }

  async function openSelectedProject(
    defaults: ProjectDefaults = {
      provider: activeProvider.id,
      model: activeProvider.model,
    },
  ): Promise<void> {
    const path = projectPath.trim();
    if (!path || openingProject) {
      projectError = "Enter the absolute path to a Git repository.";
      return;
    }
    openingProject = true;
    projectError = "";
    try {
      const project = await openRepository(path, defaults);
      selectProject(project);
      rememberedProjects = [
        {
          project,
          lastOpenedAtMs: Date.now(),
          available: true,
        },
        ...rememberedProjects.filter(
          (candidate) => candidate.project.projectId !== project.projectId,
        ),
      ].slice(0, 12);
      projectDialogOpen = false;
      toast = `${project.displayName} is ready on ${project.branch ?? "detached HEAD"}`;
      window.setTimeout(() => (toast = ""), 3600);
    } catch (error) {
      projectError =
        error instanceof Error
          ? error.message
          : "Kiln could not open this repository.";
    } finally {
      openingProject = false;
    }
  }

  async function openRememberedProject(
    remembered: RememberedProject,
  ): Promise<void> {
    if (!remembered.available) {
      showProjectDialog(remembered);
      return;
    }
    projectPath = remembered.project.root;
    await openSelectedProject(remembered.project.defaults);
  }

  function repositoryStatusLabel(project?: ProjectSnapshot): string {
    if (!project) return "Choose a Git repository";
    const count =
      project.status.staged +
      project.status.modified +
      project.status.untracked +
      project.status.conflicts;
    if (project.status.conflicts) {
      return `${project.status.conflicts} conflict${project.status.conflicts === 1 ? "" : "s"}`;
    }
    return count === 0
      ? "Working tree clean"
      : `${count} working-tree change${count === 1 ? "" : "s"}`;
  }

  async function appendApplicationEvents(
    payloads: readonly ApplicationEvent[],
    metadata: EventMetadata = {},
  ): Promise<void> {
    projection = await taskHistory.append(
      payloads,
      metadata,
      persistApplicationEvents,
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
    if (!activeProject) {
      showProjectDialog();
      return;
    }
    if (!content || running || submitting || !historyReady) return;
    submitting = true;

    const messageId = crypto.randomUUID();
    const turnId = crypto.randomUUID();
    const assistantMessageId = crypto.randomUUID();
    const commandId = `command:${turnId}`;
    try {
      try {
        await appendApplicationEvents(
          [
            {
              type: "message_added",
              data: { messageId, role: "user", content },
            },
            {
              type: "turn_started",
              data: { turnId },
            },
          ],
          { causationId: commandId, correlationId: turnId },
        );
      } catch {
        toast = "Kiln couldn’t save this turn. Your draft is still here.";
        window.setTimeout(() => (toast = ""), 4200);
        return;
      }
      draft = "";
      activeTurnId = turnId;

      const requestMessages: ChatMessage[] = projection.messages
        .map(({ role, content: messageContent }) => ({
          role,
          content: messageContent,
        }));

      try {
        await executeTurnStreaming(
          {
            provider: activeProvider.id,
            credentials: {
              apiKey: activeProvider.apiKey || undefined,
            },
            baseUrl: activeProvider.baseUrl || undefined,
            model: activeProvider.model,
            messages: requestMessages,
            maxOutputTokens: 4096,
          },
          { turnId, assistantMessageId },
          async (events) => {
            await appendApplicationEvents(events, {
              causationId: commandId,
              correlationId: turnId,
            });
          },
        );
      } catch {
        toast =
          "Kiln stopped the stream because its next event could not be saved.";
        window.setTimeout(() => (toast = ""), 5200);
      }
    } finally {
      activeTurnId = undefined;
      submitting = false;
    }
  }

  async function cancelActiveTurn(): Promise<void> {
    if (!activeTurnId) return;
    const accepted = await cancelTurn(activeTurnId);
    toast = accepted ? "Stopping this turn…" : "This turn has already stopped.";
    window.setTimeout(() => (toast = ""), 2400);
  }

  async function inspectWorkspace(): Promise<void> {
    if (!activeProject || inspectingWorkspace) {
      if (!activeProject) showProjectDialog();
      return;
    }
    inspectingWorkspace = true;
    inspectorTab = "activity";
    const toolCallId = `tool:${crypto.randomUUID()}`;
    try {
      const result = await executeVisibleRepositoryTool(
        activeProject.projectId,
        toolCallId,
        {
          tool: "search_files",
          input: { pattern: "*", maxResults: 100 },
        },
        async (events) => {
          await appendApplicationEvents(events, {
            causationId: toolCallId,
            correlationId: toolCallId,
          });
        },
      );
      if (result.tool === "search_files") {
        toast = `Inspected ${result.result.matches.length} workspace files`;
      }
    } catch (error) {
      toast =
        error instanceof Error
          ? error.message
          : "Kiln could not inspect this workspace.";
    } finally {
      inspectingWorkspace = false;
      window.setTimeout(() => (toast = ""), 3600);
    }
  }

  async function loadFileForEdit(): Promise<void> {
    if (!activeProject || !editPath.trim() || editingWorkspace) return;
    editingWorkspace = true;
    const toolCallId = `tool:${crypto.randomUUID()}`;
    try {
      const result = await executeVisibleRepositoryTool(
        activeProject.projectId,
        toolCallId,
        {
          tool: "read_file",
          input: { path: editPath.trim(), lineCount: 1000 },
        },
        async (events) => {
          await appendApplicationEvents(events, {
            causationId: toolCallId,
            correlationId: toolCallId,
          });
        },
      );
      if (result.tool === "read_file") {
        if (result.result.truncated) {
          throw new Error(
            "This file exceeds the bounded editor view, so Kiln will not replace it as a whole.",
          );
        }
        editPath = result.result.path;
        editContent = result.result.content;
        editExpectedSha256 = result.result.sha256;
        editLoadedPath = result.result.path;
        toast = `Loaded ${result.result.path} for version-checked editing`;
      }
    } catch (error) {
      editContent = "";
      editExpectedSha256 = undefined;
      editLoadedPath = undefined;
      toast =
        error instanceof Error
          ? `${error.message} If this path is genuinely new, you can still enter its initial contents.`
          : "Kiln could not load this file.";
    } finally {
      editingWorkspace = false;
      window.setTimeout(() => (toast = ""), 4200);
    }
  }

  async function applyWorkspaceEdit(): Promise<void> {
    if (!activeProject || !editPath.trim() || editingWorkspace) return;
    editingWorkspace = true;
    const toolCallId = `tool:${crypto.randomUUID()}`;
    const requestedPath = editPath.trim();
    try {
      const result = await executeVisibleRepositoryTool(
        activeProject.projectId,
        toolCallId,
        {
          tool: "write_file",
          input: {
            path: requestedPath,
            content: editContent,
            expectedSha256:
              editLoadedPath === requestedPath
                ? editExpectedSha256
                : undefined,
          },
        },
        async (events) => {
          await appendApplicationEvents(events, {
            causationId: toolCallId,
            correlationId: toolCallId,
          });
        },
      );
      if (result.tool === "write_file") {
        latestWriteDiff = result.result.unifiedDiff;
        const additions = latestWriteDiff
          .split("\n")
          .filter((line) => line.startsWith("+") && !line.startsWith("+++")).length;
        const deletions = latestWriteDiff
          .split("\n")
          .filter((line) => line.startsWith("-") && !line.startsWith("---")).length;
        files = [
          { path: result.result.path, additions, deletions },
          ...files.filter((file) => file.path !== result.result.path),
        ];
        editPath = result.result.path;
        editExpectedSha256 = result.result.afterSha256;
        editLoadedPath = result.result.path;
        inspectorTab = "changes";
        toast = `${result.result.created ? "Created" : "Updated"} ${result.result.path}; review the recorded diff`;
      }
    } catch (error) {
      toast =
        error instanceof Error
          ? error.message
          : "Kiln could not apply this workspace edit.";
    } finally {
      editingWorkspace = false;
      window.setTimeout(() => (toast = ""), 4200);
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
      <span>{activeProject?.displayName ?? "No repository"}</span>
      <span class="slash">/</span>
      <span>{activeProject?.branch ?? "detached HEAD"}</span>
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

      <button
        class="project-switcher"
        class:empty={!activeProject}
        type="button"
        onclick={() => showProjectDialog()}
      >
        <span class="project-icon">⌁</span>
        <span>
          <strong>{activeProject?.displayName ?? "Open repository"}</strong>
          <small>{repositoryStatusLabel(activeProject)}</small>
        </span>
        <span class="chevron">⌄</span>
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
            <button
              class="secondary-button"
              type="button"
              onclick={() => showProjectDialog()}
            >
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
                  {activeProject?.branch ?? "No repository"}
                </div>
                <span>
                  {activeProject
                    ? `Direct workspace · ${repositoryStatusLabel(activeProject)}`
                    : "Open a Git repository before starting a task"}
                </span>
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
                  placeholder={historyReady
                    ? activeProject
                      ? "Ask Kiln to inspect, change, explain, or plan…"
                      : "Open a Git repository to start a task…"
                    : "Restoring local task history…"}
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
                      type={running ? "button" : "submit"}
                      disabled={running
                        ? !activeTurnId
                        : !draft.trim() ||
                          submitting ||
                          !historyReady ||
                          !activeProject}
                      aria-label={running ? "Stop turn" : "Send message"}
                      onclick={(event) => {
                        if (running) {
                          event.preventDefault();
                          void cancelActiveTurn();
                        }
                      }}
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
                  <span>+{changeAdditions}</span>
                  <span>−{changeDeletions}</span>
                </div>
              </div>

              <div class="workspace-edit-card">
                <div>
                  <strong>Version-checked workspace edit</strong>
                  <span>Read first, edit locally, then confirm the exact path in a native dialog.</span>
                </div>
                <label>
                  <span>Relative file path</span>
                  <input
                    bind:value={editPath}
                    placeholder="src/example.ts"
                    disabled={editingWorkspace || !activeProject}
                  />
                </label>
                <div class="workspace-edit-actions">
                  <button
                    class="secondary-button"
                    type="button"
                    disabled={editingWorkspace || !activeProject || !editPath.trim()}
                    onclick={() => void loadFileForEdit()}
                  >
                    Read current file
                  </button>
                  <span>
                    {editExpectedSha256 && editLoadedPath === editPath.trim()
                      ? "Current version locked"
                      : "New file mode"}
                  </span>
                </div>
                <label>
                  <span>Complete UTF-8 contents</span>
                  <textarea
                    bind:value={editContent}
                    rows="8"
                    disabled={editingWorkspace || !activeProject}
                    placeholder="The complete replacement contents…"
                  ></textarea>
                </label>
                <button
                  class="primary-button"
                  type="button"
                  disabled={editingWorkspace || !activeProject || !editPath.trim()}
                  onclick={() => void applyWorkspaceEdit()}
                >
                  {editingWorkspace ? "Working…" : "Confirm and apply edit"}
                </button>
              </div>

              {#if inspector.artifacts.length}
                <div class="artifact-strip" aria-label="Recorded task artifacts">
                  {#each inspector.artifacts as artifact}
                    <div class={`artifact-chip ${artifact.kind}`}>
                      <span>{artifact.kind.replaceAll("_", " ")}</span>
                      <strong>{artifact.label}</strong>
                    </div>
                  {/each}
                </div>
              {/if}

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
                    <strong>{files[0]?.path.split("/").at(-1) ?? "No changed file"}</strong>
                  </div>
                  <span>•••</span>
                </div>
                {#if latestWriteDiff}
                  <pre class="real-diff" aria-label="Real workspace diff">{latestWriteDiff}</pre>
                {:else if !activeProject}
                <div class="diff-code" aria-label="Code change preview">
                  <div class="code-line context">
                    <span class="line-number">41</span>
                    <code>&lt;div class="provider-card"&gt;</code>
                  </div>
                  <div class="code-line removed">
                    <span class="line-number">42</span>
                    <code>- &lt;span&gt;Connected&lt;/span&gt;</code>
                  </div>
                  <div class="code-line added">
                    <span class="line-number">42</span>
                    <code>+ &lt;strong&gt;Ready for this session&lt;/strong&gt;</code>
                  </div>
                  <div class="code-line added">
                    <span class="line-number">43</span>
                    <code>+ &lt;small&gt;Connected · 42 ms&lt;/small&gt;</code>
                  </div>
                  <div class="code-line context">
                    <span class="line-number">44</span>
                    <code>&lt;/div&gt;</code>
                  </div>
                </div>
                {:else}
                  <p class="empty-diff">Apply an approved workspace edit to review its exact diff here.</p>
                {/if}
              </div>

              {#if !activeProject}
              <div class="review-actions">
                <button class="secondary-button" type="button">Discard</button>
                <button class="primary-button" type="button">
                  Review changes
                  <span>→</span>
                </button>
              </div>
              {/if}
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
                  <button
                    class="secondary-button"
                    type="button"
                    disabled={inspectingWorkspace || !activeProject}
                    onclick={() => void inspectWorkspace()}
                  >
                    {inspectingWorkspace ? "Inspecting…" : "Inspect workspace"}
                  </button>
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
            <div class="eyebrow">
              Living product roadmap · revision {roadmapRevision} ·
              {roadmapLastReviewed}
            </div>
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

  {#if projectDialogOpen}
    <button
      class="dialog-scrim"
      type="button"
      aria-label="Close repository picker"
      onclick={() => !openingProject && (projectDialogOpen = false)}
    ></button>
    <div
      class="project-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="project-dialog-title"
    >
      <header>
        <div>
          <span class="eyebrow">Direct workspace</span>
          <h2 id="project-dialog-title">Open a Git repository</h2>
        </div>
        <button
          class="dialog-close"
          type="button"
          aria-label="Close repository picker"
          disabled={openingProject}
          onclick={() => (projectDialogOpen = false)}
        >×</button>
      </header>

      {#if rememberedProjects.length}
        <div class="recent-projects">
          <span class="dialog-label">Recent repositories</span>
          {#each rememberedProjects as remembered}
            <button
              class:unavailable={!remembered.available}
              class:active={remembered.project.projectId === activeProject?.projectId}
              type="button"
              disabled={openingProject}
              onclick={() => void openRememberedProject(remembered)}
            >
              <span class="project-icon">⌁</span>
              <span>
                <strong>{remembered.project.displayName}</strong>
                <small>
                  {remembered.available
                    ? `${remembered.project.branch ?? "detached HEAD"} · ${repositoryStatusLabel(remembered.project)}`
                    : "Repository unavailable"}
                </small>
              </span>
              <em>{remembered.available ? "Open" : "Locate"}</em>
            </button>
          {/each}
        </div>
      {/if}

      <form
        onsubmit={(event) => {
          event.preventDefault();
          void openSelectedProject();
        }}
      >
        <label>
          <span class="dialog-label">Absolute repository path</span>
          <input
            bind:value={projectPath}
            type="text"
            spellcheck="false"
            placeholder={isDesktopRuntime()
              ? "D:\\Projects\\kiln or /home/you/kiln"
              : "D:\\Projects\\kiln"}
            disabled={openingProject}
          />
        </label>
        <p class="project-safety">
          Kiln resolves the repository root with Git, keeps ownership checks on,
          and remembers only workspace metadata and safe defaults.
        </p>
        {#if projectError}
          <p class="project-error" role="alert">{projectError}</p>
        {/if}
        <div class="dialog-actions">
          <button
            class="secondary-button"
            type="button"
            disabled={openingProject}
            onclick={() => (projectDialogOpen = false)}
          >Cancel</button>
          <button
            class="primary-button"
            type="submit"
            disabled={openingProject || !projectPath.trim()}
          >
            {openingProject ? "Inspecting repository…" : "Open repository"}
            {#if !openingProject}<span>→</span>{/if}
          </button>
        </div>
      </form>
    </div>
  {/if}

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
