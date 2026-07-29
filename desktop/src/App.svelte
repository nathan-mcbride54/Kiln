<script lang="ts">
  import { onMount } from "svelte";
  import {
    cancelTurn,
    canonicalProviderOrigin,
    deleteProviderCredential,
    executeVisibleRepositoryTool,
    executeTurnStreaming,
    isDesktopRuntime,
    listProviderCredentials,
    listRememberedProjects,
    listProviderCapabilities,
    loadApplicationEvents,
    openRepository,
    persistApplicationEvents,
    saveProviderCredential,
    testProviderConnection,
    usableProviderCredential,
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
    roadmapCurrentHorizon,
    roadmapLastReviewed,
    roadmapRevision,
  } from "./lib/roadmap.generated";
  import type {
    ChatMessage,
    ConnectionProbe,
    ConnectionProbeKind,
    ProjectDefaults,
    ProjectSnapshot,
    ProviderCapabilities,
    ProviderConfig,
    ProviderCredentialProfile,
    ProviderId,
    RememberedProject,
  } from "./lib/types";

  type View = "workbench" | "connections" | "roadmap";
  type InspectorTab = "changes" | "activity";
  type TaskState = "working" | "review" | "queued" | "done";
  type BooleanCapability =
    | "customBaseUrl"
    | "modelDiscovery"
    | "streaming"
    | "toolCalling"
    | "systemMessages"
    | "temperature"
    | "customHeaders";
  const taskStreamId = "task:recorded-replay";
  const diagnosticKinds: ConnectionProbeKind[] = [
    "reachability",
    "authentication",
    "model_discovery",
    "streaming",
    "tool_compatibility",
  ];
  const capabilityRows: { label: string; key: BooleanCapability }[] = [
    { label: "Custom endpoint", key: "customBaseUrl" },
    { label: "Model discovery", key: "modelDiscovery" },
    { label: "Streaming", key: "streaming" },
    { label: "Tool calling", key: "toolCalling" },
    { label: "System messages", key: "systemMessages" },
    { label: "Temperature control", key: "temperature" },
    { label: "Custom headers", key: "customHeaders" },
  ];

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
  let credentialProfiles: ProviderCredentialProfile[] = [];
  let credentialBusy: ProviderId | undefined;
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
    const [providerResult, credentialResult, historyResult, projectsResult] =
      await Promise.allSettled([
      listProviderCapabilities(),
      listProviderCredentials(),
      isDesktopRuntime()
        ? loadApplicationEvents(taskStreamId)
        : Promise.resolve(initialSessionEvents),
      listRememberedProjects(),
      ]);

    capabilities =
      providerResult.status === "fulfilled" ? providerResult.value : [];
    if (capabilities.length) {
      providers = hydrateProviderContracts(providers, capabilities);
    }
    if (credentialResult.status === "fulfilled") {
      credentialProfiles = credentialResult.value;
      applyCredentialProfiles();
    }

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

  function hydrateProviderContracts(
    current: ProviderConfig[],
    contracts: ProviderCapabilities[],
  ): ProviderConfig[] {
    return contracts.flatMap((contract) => {
      const provider = current.find((candidate) => candidate.id === contract.provider);
      if (!provider) return [];
      return [{
        ...provider,
        name: contract.displayName,
        protocol: protocolLabel(contract),
        baseUrl: contract.defaultBaseUrl,
        apiKeyRequired: contract.apiKeyRequired,
        capabilities: contract,
        diagnostics: undefined,
      }];
    });
  }

  function protocolLabel(contract: ProviderCapabilities): string {
    if (contract.protocol === "open_ai_responses") return "Responses API";
    if (contract.protocol === "anthropic_messages") return "Messages API";
    return "OpenAI-compatible";
  }

  function capabilityFor(
    provider: ProviderConfig,
  ): ProviderCapabilities | undefined {
    return provider.capabilities ??
      capabilities.find((contract) => contract.provider === provider.id);
  }

  function supportsCapability(
    provider: ProviderConfig,
    capability: BooleanCapability,
  ): boolean {
    return Boolean(capabilityFor(provider)?.[capability]);
  }

  function applyCredentialProfiles(): void {
    providers = providers.map((provider) => {
      const profile = credentialProfileFor(provider);
      return {
        ...provider,
        credentialRef: profile?.credentialRef,
        credentialBackend: profile?.backend,
        credentialOrigin: profile?.origin,
        credentialBindingState: profile?.bindingState,
      };
    });
  }

  function credentialProfileFor(
    provider: ProviderConfig,
  ): ProviderCredentialProfile | undefined {
    const candidates = credentialProfiles.filter(
      (profile) => profile.provider === provider.id,
    );
    const configuredOrigin = canonicalProviderOrigin(provider.baseUrl);
    return candidates.find((profile) =>
      profile.bindingState === "bound" &&
      Boolean(configuredOrigin) &&
      canonicalProviderOrigin(profile.origin ?? "") === configuredOrigin
    ) ??
      candidates.find((profile) =>
        profile.bindingState === "bound" && !profile.origin
      ) ??
      candidates.find((profile) => profile.bindingState === "rebind_required") ??
      candidates[0];
  }

  function updateProviderBaseUrl(id: ProviderId, baseUrl: string): void {
    const provider = providers.find((candidate) => candidate.id === id);
    if (!provider) return;
    const updated = { ...provider, baseUrl };
    const matchingProfile = credentialProfileFor(updated);
    patchProvider(id, {
      baseUrl,
      credentialRef: matchingProfile?.credentialRef ?? provider.credentialRef,
      credentialBackend:
        matchingProfile?.backend ?? provider.credentialBackend,
      credentialOrigin: matchingProfile?.origin ?? provider.credentialOrigin,
      credentialBindingState:
        matchingProfile?.bindingState ?? provider.credentialBindingState,
      state: "untested",
      latency: undefined,
      message: undefined,
      diagnostics: undefined,
    });
  }

  function updateProviderModel(id: ProviderId, model: string): void {
    patchProvider(id, {
      model,
      state: "untested",
      latency: undefined,
      message: undefined,
      diagnostics: undefined,
    });
  }

  function destinationWarning(provider: ProviderConfig): {
    from: string;
    to: string;
    legacy: boolean;
  } | undefined {
    if (!provider.credentialRef) return undefined;
    const configuredOrigin = canonicalProviderOrigin(provider.baseUrl);
    const destination = configuredOrigin ?? "an invalid destination";
    if (provider.credentialBindingState === "rebind_required") {
      return {
        from: provider.credentialOrigin ?? "unverified legacy profile",
        to: destination,
        legacy: true,
      };
    }
    if (!supportsCapability(provider, "customBaseUrl")) return undefined;
    const credentialOrigin = provider.credentialOrigin
      ? canonicalProviderOrigin(provider.credentialOrigin) ??
        provider.credentialOrigin.trim()
      : undefined;
    if (!credentialOrigin || credentialOrigin === configuredOrigin) {
      return undefined;
    }
    return {
      from: credentialOrigin,
      to: destination,
      legacy: false,
    };
  }

  function credentialBadge(provider: ProviderConfig): string {
    if (!provider.credentialRef) return "unsaved";
    return usableProviderCredential(provider) ? "stored" : "rebind";
  }

  function diagnosticProbe(
    provider: ProviderConfig,
    kind: ConnectionProbeKind,
  ): ConnectionProbe | undefined {
    return provider.diagnostics?.probes.find((probe) => probe.kind === kind);
  }

  function diagnosticLabel(kind: ConnectionProbeKind): string {
    if (kind === "model_discovery") return "Model discovery";
    if (kind === "tool_compatibility") return "Tool compatibility";
    return `${kind[0].toUpperCase()}${kind.slice(1)}`;
  }

  function probeStatusLabel(probe?: ConnectionProbe): string {
    if (!probe) return "Not tested";
    if (probe.status === "passed") return "Passed";
    if (probe.status === "failed") return "Failed";
    if (probe.status === "unsupported") return "Unsupported";
    return "Skipped";
  }

  function connectionSummary(response: ProviderConfig["diagnostics"]): string {
    if (!response) return "";
    const passed = response.probes.filter(
      (probe) => probe.status === "passed",
    ).length;
    if (response.overall === "ready") return "All provider checks passed.";
    if (response.overall === "degraded") {
      return `Connected with ${passed} of ${response.probes.length} checks passing.`;
    }
    return "The provider is unavailable; review the failed checks.";
  }

  function reachabilityLatency(
    response: ProviderConfig["diagnostics"],
  ): number | undefined {
    return response?.probes.find(
      (probe) => probe.kind === "reachability",
    )?.latencyMs;
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

    patchProvider(id, {
      state: "testing",
      latency: undefined,
      message: "Testing five provider behaviors…",
      diagnostics: undefined,
    });

    try {
      const response = await testProviderConnection(provider);
      patchProvider(id, {
        state:
          response.overall === "ready"
            ? "ready"
            : response.overall === "degraded"
              ? "degraded"
              : "error",
        latency: reachabilityLatency(response),
        message: connectionSummary(response),
        diagnostics: response,
      });
      toast =
        response.overall === "ready"
          ? `${provider.name} is ready`
          : response.overall === "degraded"
            ? `${provider.name} connected with limits`
            : `${provider.name} is unavailable`;
    } catch (error) {
      patchProvider(id, {
        state: "error",
        diagnostics: undefined,
        message: error instanceof Error ? error.message : "Connection failed.",
      });
      toast = `Couldn’t reach ${provider.name}`;
    }

    window.setTimeout(() => (toast = ""), 3200);
  }

  async function storeCredential(id: ProviderId): Promise<void> {
    const provider = providers.find((item) => item.id === id);
    if (!provider || credentialBusy) return;
    if (!provider.apiKey.trim()) {
      patchProvider(id, {
        state: "error",
        message: "Enter a key before saving it.",
      });
      return;
    }

    credentialBusy = id;
    try {
      const profile = await saveProviderCredential(
        id,
        provider.apiKey,
        provider.baseUrl,
      );
      credentialProfiles = [
        ...credentialProfiles.filter(
          (candidate) => candidate.provider !== profile.provider,
        ),
        profile,
      ];
      patchProvider(id, {
        apiKey: "",
        credentialRef: profile.credentialRef,
        credentialBackend: profile.backend,
        credentialOrigin: profile.origin,
        credentialBindingState: profile.bindingState,
        state: "untested",
        latency: undefined,
        diagnostics: undefined,
        message: `Saved in ${credentialBackendLabel(profile)}.`,
      });
      toast = `${provider.name} credential saved securely`;
    } catch (error) {
      patchProvider(id, {
        state: "error",
        message:
          error instanceof Error
            ? error.message
            : "The credential could not be saved.",
      });
      toast = `Couldn’t save the ${provider.name} credential`;
    } finally {
      credentialBusy = undefined;
      window.setTimeout(() => (toast = ""), 3600);
    }
  }

  async function removeCredential(id: ProviderId): Promise<void> {
    const provider = providers.find((item) => item.id === id);
    if (!provider?.credentialRef || !provider.credentialBackend || credentialBusy) {
      return;
    }
    credentialBusy = id;
    const profile: ProviderCredentialProfile = {
      provider: id,
      credentialRef: provider.credentialRef,
      backend: provider.credentialBackend,
      origin: provider.credentialOrigin,
      bindingState:
        provider.credentialBindingState ?? "rebind_required",
    };
    try {
      await deleteProviderCredential(profile);
      credentialProfiles = credentialProfiles.filter(
        (candidate) => candidate.credentialRef !== profile.credentialRef,
      );
      patchProvider(id, {
        apiKey: "",
        credentialRef: undefined,
        credentialBackend: undefined,
        credentialOrigin: undefined,
        credentialBindingState: undefined,
        state: "untested",
        latency: undefined,
        diagnostics: undefined,
        message: "Stored credential removed.",
      });
      applyCredentialProfiles();
      toast = `${provider.name} credential removed`;
    } catch (error) {
      patchProvider(id, {
        state: "error",
        message:
          error instanceof Error
            ? error.message
            : "The credential could not be removed.",
      });
    } finally {
      credentialBusy = undefined;
      window.setTimeout(() => (toast = ""), 3600);
    }
  }

  function credentialBackendLabel(profile: ProviderCredentialProfile): string {
    return profile.backend === "windows_credential_manager"
      ? "Windows Credential Manager"
      : "Linux Secret Service";
  }

  async function submitPrompt(): Promise<void> {
    const content = draft.trim();
    if (!activeProject) {
      showProjectDialog();
      return;
    }
    if (!content || running || submitting || !historyReady) return;
    const activeCredentialRef = usableProviderCredential(activeProvider);
    const activeContract = capabilityFor(activeProvider);
    if (
      (activeContract?.apiKeyRequired ?? activeProvider.apiKeyRequired) &&
      !activeCredentialRef
    ) {
      activeView = "connections";
      toast = activeProvider.credentialRef
        ? `Rebind the ${activeProvider.name} credential to this destination before starting a turn.`
        : `Save a ${activeProvider.name} credential before starting a turn.`;
      window.setTimeout(() => (toast = ""), 4200);
      return;
    }
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
            credentialRef: activeCredentialRef,
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
    if (state === "degraded") return "Limited";
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
          <span class="nav-count">{roadmapCurrentHorizon}</span>
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
              Secrets stay in your operating system’s credential vault.
            </p>
          </div>
          <div class="security-pill">
            <span>⌁</span>
            <div>
              <strong>OS-backed credentials</strong>
              <small>Only opaque references enter Kiln’s application data</small>
            </div>
          </div>
        </header>

        <div class="provider-grid">
          {#each providers as provider}
            {@const contract = capabilityFor(provider)}
            {@const warning = destinationWarning(provider)}
            <form
              class="provider-card"
              class:ready={provider.state === "ready"}
              class:degraded={provider.state === "degraded"}
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
                    {#if !(contract?.apiKeyRequired ?? provider.apiKeyRequired)}
                      <small>optional</small>
                    {/if}
                  </span>
                  <div class="secret-input">
                    <input
                      type="password"
                      autocomplete="off"
                      spellcheck="false"
                      value={provider.apiKey}
                      placeholder={provider.credentialRef
                        ? "Enter a replacement key"
                        : (contract?.apiKeyRequired ?? provider.apiKeyRequired)
                          ? "Enter provider API key"
                          : "Optional bearer token"}
                      oninput={(event) =>
                        patchProvider(provider.id, {
                          apiKey: event.currentTarget.value,
                          state: "untested",
                          latency: undefined,
                          message: undefined,
                          diagnostics: undefined,
                        })}
                    />
                    <span>{credentialBadge(provider)}</span>
                  </div>
                </label>

                <label>
                  <span>
                    {contract?.customBaseUrl ? "Base URL" : "Fixed API URL"}
                  </span>
                  <input
                    class="mono-input"
                    type="url"
                    value={provider.baseUrl}
                    disabled={!contract?.customBaseUrl}
                    oninput={(event) =>
                      updateProviderBaseUrl(
                        provider.id,
                        event.currentTarget.value,
                      )}
                  />
                  <small class="field-note">
                    {contract?.customBaseUrl
                      ? "Credentials bind to this exact origin."
                      : "Pinned by the first-party provider adapter."}
                  </small>
                </label>

                <label>
                  <span>Default model</span>
                  <input
                    class="mono-input"
                    type="text"
                    value={provider.model}
                    spellcheck="false"
                    list={`models-${provider.id}`}
                    oninput={(event) =>
                      updateProviderModel(
                        provider.id,
                        event.currentTarget.value,
                      )}
                  />
                  <datalist id={`models-${provider.id}`}>
                    {#each provider.diagnostics?.models ?? [] as model}
                      <option value={model}></option>
                    {/each}
                  </datalist>
                </label>
              </div>

              {#if warning}
                <div class="destination-warning" role="alert">
                  <strong>
                    {warning.legacy
                      ? "Credential needs a destination"
                      : "Credential destination changed"}
                  </strong>
                  <span>{warning.from} → {warning.to}</span>
                  <p>
                    The stored key will not be sent there. Save a credential to
                    bind it to this origin, or remove the older profile.
                  </p>
                </div>
              {/if}

              <div
                class="diagnostics-panel"
                aria-label={`${provider.name} diagnostics`}
                aria-live="polite"
              >
                <div class="diagnostics-heading">
                  <strong>Live behavior checks</strong>
                  <span>Streaming and tools send small model requests that may use a few tokens.</span>
                </div>
                <div class="probe-list">
                  {#each diagnosticKinds as kind}
                    {@const probe = diagnosticProbe(provider, kind)}
                    <div class="probe-row">
                      <span class="probe-name">{diagnosticLabel(kind)}</span>
                      <span class={`probe-status ${probe?.status ?? "not-tested"}`}>
                        {probeStatusLabel(probe)}
                      </span>
                      <span class="probe-message">
                        {probe?.message ?? "Run a connection test to verify this behavior."}
                      </span>
                      {#if probe?.latencyMs !== undefined}
                        <strong>{probe.latencyMs} ms</strong>
                      {/if}
                    </div>
                  {/each}
                </div>
              </div>

              <div class="provider-card-footer">
                <div class="connection-result" aria-live="polite">
                  {#if provider.message}
                    <span>{provider.message}</span>
                  {:else}
                    <span>
                      {usableProviderCredential(provider)
                        ? "Credential is stored and bound to this destination"
                        : provider.credentialRef
                          ? "Stored credential is not bound to this destination"
                          : (contract?.apiKeyRequired ?? provider.apiKeyRequired)
                            ? "No credential is stored yet"
                            : "A stored credential is optional for this route"}
                    </span>
                  {/if}
                  {#if provider.latency !== undefined}
                    <strong>{provider.latency} ms</strong>
                  {/if}
                </div>
                <div class="credential-actions">
                  {#if provider.credentialRef}
                    <button
                      class="credential-button remove"
                      type="button"
                      disabled={credentialBusy === provider.id}
                      onclick={() => void removeCredential(provider.id)}
                    >
                      Remove
                    </button>
                  {/if}
                  <button
                    class="credential-button"
                    type="button"
                    disabled={!provider.apiKey.trim() || credentialBusy === provider.id}
                    onclick={() => void storeCredential(provider.id)}
                  >
                    {credentialBusy === provider.id
                      ? "Saving…"
                      : warning
                        ? "Save for this origin"
                        : provider.credentialRef
                        ? "Replace key"
                        : "Save securely"}
                  </button>
                  <button
                    class="test-button"
                    type="submit"
                    disabled={provider.state === "testing" || credentialBusy === provider.id}
                  >
                    {provider.state === "testing" ? "Testing…" : "Test connection"}
                    <span>→</span>
                  </button>
                </div>
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
            {#each capabilityRows as row}
              <div class="cap-row">
                <span>{row.label}</span>
                {#each providers as provider}
                  <span class:supported={supportsCapability(provider, row.key)}>
                    {supportsCapability(provider, row.key) ? "✓" : "—"}
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
