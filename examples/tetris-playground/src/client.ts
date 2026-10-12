import type {
  AnalysisAction,
  AnalysisActionsView,
  AnalysisSelection,
  ClosedLoopEvent,
  DemoStage,
  ManagedExecutableView,
  PlaygroundSnapshot,
  ServiceStatus,
  TetrisSessionCommand,
  SessionView,
} from "./types.js";
import { createAnalysisActionsView, matchesAnalysisSelection } from "./analysis-actions.js";

const stageOrder: readonly DemoStage[] = [
  "baseline",
  "collecting",
  "materializing",
  "analyzing",
  "activating",
  "verifying",
  "completed",
];

const commandKeys = new Map<string, TetrisSessionCommand>([
  ["ArrowLeft", "move-left"],
  ["ArrowRight", "move-right"],
  ["ArrowUp", "rotate-clockwise"],
  ["ArrowDown", "soft-drop"],
  [" ", "hard-drop"],
  ["p", "pause"],
  ["r", "restart"],
]);

type AnalysisPhase = "analysis" | "candidate" | "activation";

let latest: PlaygroundSnapshot | undefined;
let selectedExecutableDigest: string | undefined;
let renderedExecutableTable: string | undefined;
let renderedExecutableDetail: string | undefined;
let selectedServiceName: string | undefined;
let activationPending = false;
let showNotEligible = false;
let observedAnalysisEventIds = new Set<string>();
let analysisTimelineInitialized = false;
let selectedSession = "human";
let firstBotIndex = 0;
const visibleBotCount = 3;
const botCards = new Map<string, HTMLElement>();
const visibleAnalysisPhases = new Set<AnalysisPhase>([
  "analysis",
  "candidate",
  "activation",
]);
let selectedAnalysisCycle: string | undefined;
let selectedAnalysisAttempt: string | undefined;
let actionInspectionPending = false;
let actionInspectionError: string | undefined;
let inspectedAnalysis: { selection: AnalysisSelection; view: AnalysisActionsView } | undefined;
const actionRows = new Map<string, HTMLDetailsElement>();

const connection = requiredElement("connection");
const board = requiredElement("board");
const services = requiredElement("services");
const timeline = requiredElement("timeline");
const backendFailures = requiredElement("backend-failures");
const decision = requiredElement("decision");
const executables = requiredElement("executables");
const evidence = requiredElement("evidence");
const demo = requiredElement("demo");
const demoRun = requiredButton("demo-run");
const demoCancel = requiredButton("demo-cancel");
const grafanaLink = requiredAnchor("grafana-link");
const healthDialog = requiredDialog("health-dialog");
const healthTitle = requiredElement("health-title");
const healthDetail = requiredElement("health-detail");
const healthClose = requiredButton("health-close");
const executableDialog = requiredDialog("executable-dialog");
const executableTitle = requiredElement("executable-title");
const executableDetailContainer = requiredElement("executable-detail");
const executableClose = requiredButton("executable-close");
const filterNotEligible = requiredInput("filter-not-eligible");
const analysisFilterCount = requiredElement("analysis-filter-count");
const analysisFeedStatus = requiredElement("analysis-feed-status");
const botGrid = requiredElement("bot-grid");
const botStart = requiredButton("bots-start");
const botStop = requiredButton("bots-stop");
const botToggle = requiredButton("bots-toggle");
const botPagination = requiredElement("bots-pagination");
const botRange = requiredElement("bots-range");
const botPrevious = requiredButton("bots-previous");
const botNext = requiredButton("bots-next");
const analysisCycle = requiredSelect("analysis-cycle");
const analysisAttempt = requiredSelect("analysis-attempt");
const analysisRefresh = requiredButton("analysis-refresh");
const analysisActions = requiredElement("analysis-actions");
const analysisActionStatus = requiredElement("analysis-action-status");
const analysisLatestAction = requiredElement("analysis-latest-action");
analysisCycle.addEventListener("change", () => {
  selectedAnalysisCycle = analysisCycle.value;
  selectedAnalysisAttempt = undefined;
  inspectedAnalysis = undefined;
  actionInspectionError = undefined;
  if (latest !== undefined) renderAnalysisActions(latest);
  void refreshAnalysisActions();
});
analysisAttempt.addEventListener("change", () => {
  selectedAnalysisAttempt = analysisAttempt.value;
  inspectedAnalysis = undefined;
  actionInspectionError = undefined;
  if (latest !== undefined) renderAnalysisActions(latest);
  void refreshAnalysisActions();
});
analysisRefresh.addEventListener("click", () => void refreshAnalysisActions());
window.setInterval(() => void refreshAnalysisActions(), 5_000);
const sessionSelectElement = requiredElement("session-select");
if (!(sessionSelectElement instanceof HTMLSelectElement)) {
  throw new Error("Element '#session-select' must be a selector.");
}
const sessionSelect = sessionSelectElement;
sessionSelect.addEventListener("change", () => {
  selectedSession = sessionSelect.value;
  if (latest !== undefined) renderDecision(latest);
});
botStart.addEventListener("click", () => void post("/api/bots/start"));
botStop.addEventListener("click", () => void post("/api/bots/stop"));
botPrevious.addEventListener("click", () => {
  firstBotIndex -= 1;
  if (latest !== undefined) renderBots(latest);
});
botNext.addEventListener("click", () => {
  firstBotIndex += 1;
  if (latest !== undefined) renderBots(latest);
});
botToggle.addEventListener("click", () => {
  botGrid.hidden = !botGrid.hidden;
  botPagination.hidden = botGrid.hidden;
  botToggle.textContent = botGrid.hidden ? "Expand" : "Collapse";
  botToggle.setAttribute("aria-expanded", String(!botGrid.hidden));
});
botGrid.addEventListener("click", (event) => {
  if (!(event.target instanceof HTMLElement)) return;
  const button = event.target.closest<HTMLButtonElement>("[data-bot-action]");
  const id = button?.dataset.botId;
  const action = button?.dataset.botAction;
  if (id !== undefined && (action === "start" || action === "stop")) {
    void post(`/api/bots/${encodeURIComponent(id)}/${action}`);
  }
});

for (const button of document.querySelectorAll<HTMLButtonElement>(
  "[data-command]",
)) {
  button.addEventListener("click", () => {
    const command = button.dataset.command as TetrisSessionCommand | undefined;
    if (command !== undefined) void sendCommand(command);
  });
}

for (const input of document.querySelectorAll<HTMLInputElement>(
  "[data-analysis-phase]",
)) {
  input.addEventListener("change", () => {
    const phase = input.dataset.analysisPhase;
    if (!isAnalysisPhase(phase)) return;
    if (input.checked) visibleAnalysisPhases.add(phase);
    else visibleAnalysisPhases.delete(phase);
    if (latest !== undefined) renderAnalysisTimeline(latest.timeline);
  });
}

filterNotEligible.addEventListener("change", () => {
  showNotEligible = filterNotEligible.checked;
  if (latest !== undefined) renderAnalysisTimeline(latest.timeline);
});

services.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) return;
  const button = target.closest<HTMLElement>("[data-service-name]");
  const serviceName = button?.dataset.serviceName;
  if (serviceName === undefined || latest === undefined) return;
  selectedServiceName = serviceName;
  renderHealthDetail(latest);
  if (!healthDialog.open) healthDialog.showModal();
});

executables.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) return;
  const page = target.closest<HTMLElement>("[data-history-page]");
  if (page !== null && latest !== undefined) {
    const cursor = page.dataset.historyPage === "older" ? latest.executables.nextCursor : undefined;
    void post("/api/executables/history"
      + (cursor === undefined ? "" : `?cursor=${encodeURIComponent(cursor)}`))
      .catch((error: unknown) => {
        window.alert(error instanceof Error ? error.message : String(error));
      });
    return;
  }
  const option = target.closest<HTMLElement>("[data-executable-select]");
  if (option?.dataset.executableSelect === undefined || latest === undefined) {
    return;
  }
  selectedExecutableDigest = option.dataset.executableSelect;
  renderExecutables(latest);
  renderExecutableDialog(latest);
  if (!executableDialog.open) executableDialog.showModal();
});

executableDetailContainer.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) return;
  const activate = target.closest<HTMLElement>("[data-executable-activate]");
  if (activate?.dataset.executableActivate !== undefined) {
    void activateCandidate(activate.dataset.executableActivate);
  }
});

healthClose.addEventListener("click", () => healthDialog.close());
healthDialog.addEventListener("click", (event) => {
  if (event.target === healthDialog) healthDialog.close();
});
executableClose.addEventListener("click", () => executableDialog.close());
executableDialog.addEventListener("click", (event) => {
  if (event.target === executableDialog) executableDialog.close();
});

demoRun.addEventListener("click", () => void post("/api/demo/run"));
demoCancel.addEventListener("click", () => void post("/api/demo/cancel"));

window.addEventListener("keydown", (event) => {
  if (
    event.target instanceof HTMLInputElement
    || event.target instanceof HTMLTextAreaElement
    || event.target instanceof HTMLSelectElement
    || (event.target instanceof HTMLElement && event.target.isContentEditable)
    || (event.target instanceof HTMLElement && event.target.closest("#bots-pagination") !== null)
    || (event.target instanceof HTMLElement && event.target.closest("#analysis-action-panel") !== null)
    || executableDialog.open || healthDialog.open
    || (event.target instanceof HTMLButtonElement && (event.key === " " || event.key === "Enter"))
  ) {
    return;
  }
  const command = commandKeys.get(event.key);
  if (command === undefined) return;
  event.preventDefault();
  if (command === "pause" && latest?.human.state.paused === true) {
    void sendCommand("resume");
  } else {
    void sendCommand(command);
  }
});

const events = new EventSource("/api/events");
events.addEventListener("open", () => {
  connection.textContent = "Live";
  connection.className = "connection live";
});
events.addEventListener("error", () => {
  connection.textContent = "Reconnecting";
  connection.className = "connection reconnecting";
});
events.addEventListener("state", (event) => {
  latest = JSON.parse((event as MessageEvent<string>).data) as PlaygroundSnapshot;
  render(latest);
});

function render(snapshot: PlaygroundSnapshot): void {
  grafanaLink.href = snapshot.urls.grafana;
  renderBoard(snapshot);
  renderBots(snapshot);
  renderDecision(snapshot);
  renderExecutables(snapshot);
  renderAnalysisActions(snapshot);
  renderServices(snapshot.services);
  renderEvidence(snapshot);
  requiredElement("benchmark-panel").hidden = snapshot.mode !== "benchmark";
  if (snapshot.mode === "benchmark") renderDemo(snapshot);
  renderBackendFailures(snapshot);
  analysisFeedStatus.textContent = snapshot.activity.status === "stale"
    ? "Stale / partial telemetry — retained history"
    : snapshot.activity.status === "loading" ? "Waiting for telemetry" : "Live telemetry";
  analysisFeedStatus.title = snapshot.activity.status === "stale"
    ? `${snapshot.activity.error ?? "Activity collection failed."} Last complete poll: ${
      snapshot.activity.lastSuccessfulPollAt ?? "none"
    }`
    : "Bounded observed history; old rows retain their original timestamps.";
  analysisFeedStatus.dataset.status = snapshot.activity.status;
  renderAnalysisTimeline(snapshot.timeline);
  renderHealthDetail(snapshot);
  if (executableDialog.open) renderExecutableDialog(snapshot);
  const game = snapshot.human.state;
  text("score", game.score);
  text("lines", game.lines);
  text("level", game.level);
  text("piece", game.activePiece);
  text("game-status", humanize(game.status));
  text("app-drop-interval", game.dropInterval.intervalMs);
  text(
    "app-decision-source",
    game.dropInterval.status,
  );
}

function requiredSelect(id: string): HTMLSelectElement {
  const element = requiredElement(id);
  if (!(element instanceof HTMLSelectElement)) throw new Error(`Element '#${id}' must be a selector.`);
  return element;
}

function analysisCycleKey(action: Pick<AnalysisSelection, "contractDigest" | "cycleId">): string {
  return JSON.stringify([action.contractDigest, action.cycleId]);
}

function renderAnalysisActions(snapshot: PlaygroundSnapshot): void {
  const globalView = snapshot.analysisActions;
  const cycles = new Map<string, AnalysisAction>();
  for (const action of [...globalView.actions, ...(inspectedAnalysis?.view.actions ?? [])]) {
    const key = analysisCycleKey(action);
    if (!cycles.has(key)) cycles.set(key, action);
  }
  if (selectedAnalysisCycle === undefined) selectedAnalysisCycle = cycles.keys().next().value;
  const options = [...cycles].map(([value, action]) => ({
    value, label: `${action.cycleId} · ${shortDigest(action.contractDigest)}`,
  }));
  if (selectedAnalysisCycle !== undefined && !cycles.has(selectedAnalysisCycle)) {
    options.push({ value: selectedAnalysisCycle, label: "Selected cycle outside retained sample" });
  }
  updateActionOptions(analysisCycle, options, selectedAnalysisCycle, "No observed cycles");
  const cycleActions = [...globalView.actions, ...(inspectedAnalysis?.view.actions ?? [])].filter((action) =>
    analysisCycleKey(action) === selectedAnalysisCycle);
  const attempts = [...new Set(cycleActions.map((action) => action.attemptId))];
  if (selectedAnalysisAttempt === undefined) selectedAnalysisAttempt = attempts[0];
  if (selectedAnalysisAttempt !== undefined && !attempts.includes(selectedAnalysisAttempt)) {
    attempts.push(selectedAnalysisAttempt);
  }
  updateActionOptions(analysisAttempt, attempts.map((value) => ({ value, label: value })),
    selectedAnalysisAttempt, "No observed attempts");
  let view = globalView;
  if (inspectedAnalysis !== undefined
    && analysisCycleKey(inspectedAnalysis.selection) === selectedAnalysisCycle
    && inspectedAnalysis.selection.attemptId === selectedAnalysisAttempt) {
    view = createAnalysisActionsView({
      logs: [], observations: globalView.observations,
      previous: inspectedAnalysis.view, selection: inspectedAnalysis.selection,
      now: new Date(inspectedAnalysis.view.lastSuccessfulPollAt ?? snapshot.observedAt),
      ...(inspectedAnalysis.view.error === undefined ? {} : { error: inspectedAnalysis.view.error }),
    });
    inspectedAnalysis = { ...inspectedAnalysis, view };
  }
  const actions = view.actions.filter((action) =>
    analysisCycleKey(action) === selectedAnalysisCycle && action.attemptId === selectedAnalysisAttempt);
  analysisRefresh.disabled = selectedAnalysisCycle === undefined
    || selectedAnalysisAttempt === undefined || actionInspectionPending;
  analysisActionStatus.dataset.status = actionInspectionError === undefined ? view.status : "stale";
  analysisActionStatus.textContent = [
    actionInspectionError ?? (view.status === "stale"
      ? view.error ?? "Action telemetry stale; retaining latest observed facts."
      : view.status === "loading" ? "Waiting for action telemetry" : "Action telemetry current"),
    `Last successful poll: ${view.lastSuccessfulPollAt ?? "none"}`,
    `${actions.length} observed actions; limits ${view.observationLimit} events / ${view.actionLimit} actions`,
    ...(view.limited ? ["Sample limit reached / retained history truncated"] : []),
  ].join(" · ");
  const newest = actions[0];
  analysisLatestAction.textContent = newest === undefined
    ? "No actions observed for this selection."
    : `Latest observed action: ${newest.kind} · ${newest.outcome ?? newest.status} · ${newest.latestObservedAt}`;
  const keys = new Set(actions.map((action) => action.key));
  for (const [key, row] of actionRows) {
    if (keys.has(key)) continue;
    row.remove();
    actionRows.delete(key);
  }
  actions.forEach((action, index) => {
    let row = actionRows.get(action.key);
    if (row === undefined) {
      row = document.createElement("details");
      row.className = "analysis-action";
      row.dataset.actionId = action.actionId;
      row.dataset.actionKey = action.key;
      row.append(document.createElement("summary"), document.createElement("div"));
      actionRows.set(action.key, row);
    }
    row.dataset.status = action.status;
    const summary = row.querySelector("summary")!;
    const title = `${action.kind} · ${action.outcome ?? action.status}`;
    if (summary.textContent !== title) summary.textContent = title;
    const detail = row.lastElementChild!;
    const html = actionDetail(action, snapshot.urls.grafana);
    if (detail.innerHTML !== html) detail.innerHTML = html;
    if (analysisActions.children[index] !== row) {
      analysisActions.insertBefore(row, analysisActions.children[index] ?? null);
    }
  });
}

function updateActionOptions(
  select: HTMLSelectElement,
  options: readonly { readonly value: string; readonly label: string }[],
  selected: string | undefined,
  empty: string,
): void {
  const html = options.length === 0 ? `<option value="">${escapeHtml(empty)}</option>`
    : options.map((option) => `<option value="${escapeHtml(option.value)}">${
      escapeHtml(option.label)}</option>`).join("");
  if (select.innerHTML !== html) select.innerHTML = html;
  select.value = selected ?? "";
}

function actionDetail(action: AnalysisAction, grafana: string): string {
  const fields: readonly [string, string | number | undefined][] = [
    ["Contract", action.contractName], ["Contract digest", action.contractDigest],
    ["Cycle", action.cycleId], ["Attempt", action.attemptId], ["Action", action.actionId],
    ["Start", action.startedAt ?? "Start not observed"],
    ["Completion", action.completedAt ?? action.status],
    ["Duration (ms)", action.attributes["flaggo.analysis.action.duration_ms"]],
    ["Rows", action.attributes["flaggo.analysis.result.rows"]],
    ["Bytes", action.attributes["flaggo.analysis.result.bytes"]],
    ["Query digest", action.attributes["flaggo.analysis.query.digest"]],
    ["Artifact digest", action.attributes["flaggo.analysis.artifact.digest"]],
    ["Cutoff", action.attributes["flaggo.analysis.snapshot.cutoff"]],
    ["Watermark", action.attributes["flaggo.analysis.snapshot.watermark"]],
    ["Tool call", action.attributes["flaggo.analysis.tool_call.id"]],
    ["Session digest", action.attributes["flaggo.analysis.session.id"]],
    ["Failure category", action.attributes["flaggo.failure.category"]],
  ];
  const traceUrl = new URL("explore", grafana.endsWith("/") ? grafana : `${grafana}/`);
  traceUrl.searchParams.set("left", JSON.stringify({
    datasource: "tempo", queries: [{ query: action.traceId, queryType: "traceql" }],
  }));
  return `<dl>${fields.filter(([, value]) => value !== undefined).map(([name, value]) =>
    `<div><dt>${escapeHtml(name)}</dt><dd>${escapeHtml(String(value))}</dd></div>`).join("")}</dl>`
    + (action.traceId === undefined ? "<p>Trace not observed</p>"
      : `<a class="action-trace" href="${escapeHtml(traceUrl.toString())}" target="_blank" rel="noopener noreferrer">Open trace ${escapeHtml(shortId(action.traceId))}</a>`);
}

async function refreshAnalysisActions(): Promise<void> {
  if (actionInspectionPending || selectedAnalysisCycle === undefined
    || selectedAnalysisAttempt === undefined) return;
  const [contractDigest, cycleId] = JSON.parse(selectedAnalysisCycle) as [string, string];
  const selection = { contractDigest, cycleId, attemptId: selectedAnalysisAttempt };
  const params = new URLSearchParams(selection);
  actionInspectionPending = true;
  if (latest !== undefined) renderAnalysisActions(latest);
  try {
    const response = await fetch(`/api/analysis/actions?${params}`, { signal: AbortSignal.timeout(5_000) });
    if (!response.ok) throw new Error("Action inspection backend unavailable; retaining latest observed facts.");
    const view: AnalysisActionsView = await response.json();
    if (selectedAnalysisCycle === JSON.stringify([contractDigest, cycleId])
      && selectedAnalysisAttempt === selection.attemptId) {
      const previous = inspectedAnalysis !== undefined
        && matchesAnalysisSelection(inspectedAnalysis.selection, selection)
        ? inspectedAnalysis.view : undefined;
      inspectedAnalysis = { selection, view: createAnalysisActionsView({
        logs: [], observations: view.observations, selection,
        ...(previous === undefined ? {} : { previous }),
        now: new Date(view.lastSuccessfulPollAt ?? latest?.observedAt ?? new Date().toISOString()),
        queryLimited: view.limited,
        ...(view.error === undefined ? {} : { error: view.error }),
      }) };
      actionInspectionError = undefined;
    }
  } catch {
    if (selectedAnalysisCycle === JSON.stringify([contractDigest, cycleId])
      && selectedAnalysisAttempt === selection.attemptId) {
      actionInspectionError = "Action inspection backend unavailable; retaining latest observed facts.";
    }
  } finally {
    actionInspectionPending = false;
    if (latest !== undefined) renderAnalysisActions(latest);
    if (selectedAnalysisCycle !== JSON.stringify([contractDigest, cycleId])
      || selectedAnalysisAttempt !== selection.attemptId) void refreshAnalysisActions();
  }
}

function renderBoard(snapshot: PlaygroundSnapshot): void {
  const fragment = document.createDocumentFragment();
  const game = snapshot.human.state;
  for (const row of game.board) {
    for (const cell of row) {
      const element = document.createElement("div");
      element.className = cell === null
        ? "cell"
        : `cell filled piece-${cell.toLowerCase()}`;
      fragment.append(element);
    }
  }
  board.replaceChildren(fragment);
  board.classList.toggle("paused", game.paused);
  board.classList.toggle("game-over", game.gameOver);
  const overlay = requiredElement("board-overlay");
  overlay.textContent = game.gameOver
    ? "Game over - press R"
    : game.paused
      ? "Paused"
      : "";
  overlay.classList.toggle(
    "visible",
    game.gameOver || game.paused,
  );
}

function renderDecision(snapshot: PlaygroundSnapshot): void {
  const view = selectedSession === "human" ? snapshot.human
    : snapshot.bots.find((slot) => slot.id === selectedSession)?.session;
  if (view === undefined) {
    decision.innerHTML = emptyState("Start this player to inspect its decisions.");
    return;
  }
  const value = view.decision;
  if (value === undefined) {
    decision.innerHTML = emptyState(view.error ?? "Waiting for the first Flaggo decision.");
    return;
  }
  const error = view.error ?? value.error;
  decision.innerHTML = `
    <div class="decision-result">
      <span>${view.state.dropInterval.intervalMs}</span><small>ms applied gravity</small>
    </div>
    <div class="kv-grid">
      ${kv("Source", value.source)}
      ${kv("Rule", value.rule ?? value.status)}
      ${kv("Latency", `${value.latencyMs.toFixed(1)} ms`)}
      ${kv("Received", `${value.result ?? "-"} ms / ${shortDigest(value.executableDigest)}`)}
      ${kv("Applied", shortDigest(view.state.dropInterval.executableDigest))}
      ${kv("Queued", view.state.pendingDropInterval === undefined ? "none"
        : `${view.state.pendingDropInterval.intervalMs} ms / ${shortDigest(view.state.pendingDropInterval.executableDigest)}`)}
      ${kv("Session", shortId(view.state.sessionId))}
    </div>
    ${error === undefined
      ? ""
      : `<p class="error">${escapeHtml(error)}</p>`}
  `;
}

function renderBots(snapshot: PlaygroundSnapshot): void {
  requiredElement("bots-panel").hidden = snapshot.mode !== "live";
  if (snapshot.mode !== "live") return;
  const lastStartIndex = Math.max(0, snapshot.bots.length - visibleBotCount);
  firstBotIndex = Math.max(0, Math.min(firstBotIndex, lastStartIndex));
  const visibleSlots = snapshot.bots.slice(firstBotIndex, firstBotIndex + visibleBotCount);
  const visibleIds = new Set(visibleSlots.map((slot) => slot.id));
  for (const [id, card] of botCards) {
    if (!visibleIds.has(id)) {
      card.remove();
      botCards.delete(id);
    }
  }
  const range = snapshot.bots.length === 0 ? "No autonomous sessions"
    : `Bots ${firstBotIndex + 1}-${firstBotIndex + visibleSlots.length} of ${snapshot.bots.length}`;
  if (botRange.textContent !== range) botRange.textContent = range;
  botPrevious.disabled = firstBotIndex === 0;
  botNext.disabled = firstBotIndex === lastStartIndex;
  botPagination.hidden = botGrid.hidden;
  const inspectionIds = new Set(Array.from(sessionSelect.options, (option) => option.value));
  for (const slot of snapshot.bots) {
    if (!inspectionIds.has(slot.id)) {
      const option = document.createElement("option");
      option.value = slot.id;
      option.textContent = humanize(slot.id);
      sessionSelect.append(option);
    }
  }
  for (const [index, slot] of visibleSlots.entries()) {
    let card = botCards.get(slot.id);
    if (card === undefined) {
      card = document.createElement("article");
      card.className = "bot-card";
      card.dataset.botId = slot.id;
      card.innerHTML = `
        <div class="bot-heading"><strong>${escapeHtml(humanize(slot.id))}</strong><span class="bot-status badge"></span></div>
        <div class="board bot-board" aria-label="${escapeHtml(slot.id)} board"></div>
        <p class="bot-facts"></p><p class="bot-policy"></p><p class="bot-error error" hidden></p>
        <div class="button-row">
          <button data-bot-id="${escapeHtml(slot.id)}" data-bot-action="start">Start</button>
          <button data-bot-id="${escapeHtml(slot.id)}" data-bot-action="stop">Stop</button>
        </div>`;
      botCards.set(slot.id, card);
    }
    if (botGrid.children[index] !== card) {
      botGrid.insertBefore(card, botGrid.children[index] ?? null);
    }
    const element = (selector: string): HTMLElement => {
      const value = card.querySelector<HTMLElement>(selector);
      if (value === null) throw new Error(`Missing bot element '${selector}'.`);
      return value;
    };
    element(".bot-status").textContent = humanize(slot.status);
    element(".bot-facts").textContent = slot.session === undefined ? "Not started"
      : `Game ${slot.episode} / ${slot.session.state.lines} lines / `
        + `${Math.round(slot.session.state.dropObservation.boardPressure * 100)}% pressure`;
    element(".bot-policy").textContent = slot.session === undefined ? "Real-time greedy"
      : `${slot.session.state.dropInterval.intervalMs} ms applied / ${slot.session.state.dropInterval.source}`;
    const error = element(".bot-error");
    error.hidden = slot.error === undefined;
    error.textContent = slot.error ?? "";
    const cells = document.createDocumentFragment();
    for (const row of slot.session?.state.board
      ?? Array.from({ length: 20 }, () => Array.from({ length: 10 }, () => null))) {
      for (const cell of row) {
        const value = document.createElement("div");
        value.className = cell === null ? "cell" : `cell filled piece-${cell.toLowerCase()}`;
        cells.append(value);
      }
    }
    element(".bot-board").replaceChildren(cells);
    const start = card.querySelector<HTMLButtonElement>('[data-bot-action="start"]');
    const stop = card.querySelector<HTMLButtonElement>('[data-bot-action="stop"]');
    if (start === null || stop === null) throw new Error("Bot controls are missing.");
    start.disabled = slot.status === "running" || slot.status === "stopping";
    stop.disabled = slot.status !== "running";
  }
  text("bot-count", `${snapshot.bots.filter((slot) => slot.status === "running").length} running`);
  botStart.disabled = !snapshot.bots.some((slot) => slot.status === "idle");
  botStop.disabled = !snapshot.bots.some((slot) => slot.status === "running");
  const human = snapshot.human.totals;
  const bots = snapshot.bots.reduce((total, slot) => ({
    activePlayMs: total.activePlayMs + slot.totals.activePlayMs,
    balancedPlayMs: total.balancedPlayMs + slot.totals.balancedPlayMs,
    linesCleared: total.linesCleared + slot.totals.linesCleared,
    gameOvers: total.gameOvers + slot.totals.gameOvers,
  }), { activePlayMs: 0, balancedPlayMs: 0, linesCleared: 0, gameOvers: 0 });
  const summary = (label: string, totals: SessionView["experience"]): string => {
    const balance = totals.activePlayMs === 0 ? "-"
      : `${(totals.balancedPlayMs / totals.activePlayMs * 100).toFixed(1)}%`;
    const lines = totals.activePlayMs === 0 ? "-"
      : (totals.linesCleared * 60_000 / totals.activePlayMs).toFixed(1);
    return `<div><strong>${label}</strong><span>${(totals.activePlayMs / 60_000).toFixed(1)} active min</span>`
      + `<span>${balance} balance / ${lines} lines/min / ${totals.gameOvers} game overs</span></div>`;
  };
  requiredElement("population-results").innerHTML = summary("Human", human) + summary("Greedy players", bots)
    + "<small>Observed since launch; not a held-out comparison or a human-enjoyment claim.</small>";
}

function renderExecutables(snapshot: PlaygroundSnapshot): void {
  const catalog = snapshot.executables;
  const active = catalog.activeExecutable;
  const values = executableValues(snapshot);
  if (
    selectedExecutableDigest === undefined
    || !values.some((value) =>
      value.executableDigest === selectedExecutableDigest
    )
  ) {
    selectedExecutableDigest = active?.executableDigest
      ?? catalog.history[0]?.executableDigest;
  }

  const html = active === undefined && catalog.status === "loading"
    ? emptyState("Loading executable authority.")
    : `
    ${catalog.status === "unavailable"
      ? `<p class="catalog-error">${escapeHtml(
        catalog.error ?? "Contract Service is unavailable.",
      )}</p>`
      : ""}
    <div class="executable-list">
      <div class="executable-list-heading">
        <span>Status</span>
        <span>Digest</span>
        <span>Version</span>
      </div>
      ${active === undefined
        ? ""
        : executableOption(active)}
      ${values.filter((value) => value.executableDigest !== active?.executableDigest).map((value) =>
          executableOption(value)
        ).join("")}
      ${catalog.historyCursor === undefined
        ? ""
        : `<button data-history-page="newest">Newest history</button>`}
      ${catalog.nextCursor === undefined
        ? ""
        : `<button data-history-page="older">Older executables</button>`}
    </div>
  `;
  if (html !== renderedExecutableTable) {
    executables.innerHTML = html;
    renderedExecutableTable = html;
  }
}

function renderExecutableDialog(snapshot: PlaygroundSnapshot): void {
  const selected = executableValues(snapshot).find((value) =>
    value.executableDigest === selectedExecutableDigest
  );
  const title = selected === undefined ? "Decision executable"
    : `${humanize(selected.lifecycleState)} executable`;
  if (executableTitle.textContent !== title) executableTitle.textContent = title;
  const html = selected === undefined
    ? emptyState("The selected executable is no longer available.")
    : executableDetail(selected, snapshot.executables.status);
  if (html !== renderedExecutableDetail) {
    executableDetailContainer.innerHTML = html;
    renderedExecutableDetail = html;
  }
}

function executableValues(
  snapshot: PlaygroundSnapshot,
): readonly ManagedExecutableView[] {
  const active = snapshot.executables.activeExecutable;
  return active === undefined
    ? snapshot.executables.history
    : [active, ...snapshot.executables.history.filter((value) =>
      value.executableDigest !== active.executableDigest
    )];
}

function executableOption(
  value: ManagedExecutableView,
): string {
  const selected = value.executableDigest === selectedExecutableDigest;
  return `
    <button
      class="executable-option ${selected ? "selected" : ""}"
      data-executable-select="${escapeHtml(value.executableDigest)}"
      type="button"
    >
      <span class="executable-state ${value.lifecycleState}">
        ${escapeHtml(humanize(value.lifecycleState))}
      </span>
      <code title="${escapeHtml(value.executableDigest)}">
        ${escapeHtml(shortDigest(value.executableDigest))}
      </code>
      <small>v${value.stateVersion} · ${value.executable.rules.length}r</small>
    </button>
  `;
}

function executableDetail(
  value: ManagedExecutableView,
  catalogStatus: PlaygroundSnapshot["executables"]["status"],
): string {
  const candidate = value.lifecycleState === "candidate";
  const rules = value.executable.rules;
  return `
    <div class="executable-detail-header">
      <div>
        <strong>${escapeHtml(humanize(value.lifecycleState))}</strong>
        <code class="executable-digest-full">${escapeHtml(
          value.executableDigest,
        )}</code>
      </div>
      <span class="badge">${rules.length} rules</span>
    </div>
    <div class="rule-list">
      ${rules.length === 0
        ? `<div class="rule-card"><strong>Default</strong><code>No rules; contract default applies.</code></div>`
        : rules.map((rule) => `
          <div class="rule-card">
            <strong>${escapeHtml(rule.name)}</strong>
            <code>when ${escapeHtml(rule.when.expression)}</code>
            <code>return ${escapeHtml(formatRuleReturn(rule.return))}</code>
          </div>
        `).join("")}
    </div>
    ${renderProvenance(value.provenance)}
    <div class="activation-row">
      <small class="empty">${escapeHtml(formatTime(value.createdAt))} · state v${value.stateVersion}</small>
      ${candidate
        ? `<button
            class="primary"
            data-executable-activate="${escapeHtml(value.executableDigest)}"
            ${activationPending || catalogStatus !== "ready" ? "disabled" : ""}
          >${activationPending ? "Activating…" : "Activate"}</button>`
        : ""}
    </div>
  `;
}

function renderProvenance(value: unknown): string {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return "";
  }
  const entries = Object.entries(value).filter(([, item]) =>
    typeof item === "string" || typeof item === "number"
  ).slice(0, 4);
  if (entries.length === 0) return "";
  return `
    <div class="kv-grid">
      ${entries.map(([name, item]) =>
        kv(humanize(name), shortDigest(String(item)))
      ).join("")}
    </div>
  `;
}

function renderServices(values: readonly ServiceStatus[]): void {
  services.innerHTML = values.map((value) => `
    <button
      class="health-pill ${value.status}"
      data-service-name="${escapeHtml(value.name)}"
      title="${escapeHtml(value.detail)}"
      type="button"
    >${escapeHtml(shortServiceName(value.displayName))}</button>
  `).join("");
}

function renderHealthDetail(snapshot: PlaygroundSnapshot): void {
  if (selectedServiceName === undefined) return;
  const service = snapshot.services.find((value) =>
    value.name === selectedServiceName
  );
  if (service === undefined) return;
  healthTitle.textContent = service.displayName;
  const metrics = snapshot.metrics
    .filter((value) => value.service === service.name)
    .slice(0, 12);
  healthDetail.innerHTML = `
    <p class="panel-intro">${escapeHtml(service.detail)}</p>
    <div class="health-summary">
      ${healthFact("Health", humanize(service.status))}
      ${healthFact("Process", humanize(service.process))}
      ${healthFact("Readiness", humanize(service.readiness))}
      ${healthFact(
        "Last signal",
        service.lastSignalAt === undefined
          ? "Not observed"
          : `${timeAgo(service.lastSignalAt, snapshot.observedAt)} ago`,
      )}
    </div>
    <p class="eyebrow">Live metrics</p>
    ${metrics.length === 0
      ? emptyState("No registered metrics observed for this service yet.")
      : `<div class="health-metrics">${metrics.map((value) => `
          <div class="health-metric">
            <span>${escapeHtml(humanize(
              value.name.replace(/^flaggo_/u, ""),
            ))}</span>
            <strong>${escapeHtml(formatNumber(value.value))}</strong>
          </div>
        `).join("")}</div>`}
  `;
}

function renderEvidence(snapshot: PlaygroundSnapshot): void {
  const value = snapshot.evidence;
  const inboxBatches = value.inboxRetainedBatches ?? 0;
  const observations =
    value.evidenceStoreObservations ?? value.observationsCreated ?? 0;
  evidence.innerHTML = `
    <div class="pipeline">
      ${pipelineNode(
        "OTLP Inbox",
        `${inboxBatches} batches`,
        `${formatBytes(value.inboxRetainedBytes)} retained · ${
          formatDuration(value.inboxOldestAgeMs)
        } oldest`,
        pressureClass(inboxBatches, 4, 12),
        capacityPercent(inboxBatches, 20),
      )}
      <span class="pipeline-arrow">→</span>
      ${pipelineNode(
        "Materializer",
        value.checkpointBatchId === undefined
          ? "Waiting"
          : `Checkpoint ${value.checkpointBatchId}`,
        `${value.pendingBatches ?? 0} pending · ${
          value.batchesRead ?? 0
        } read`,
        value.stale ? "pressure-medium" : "pressure-low",
        capacityPercent(value.pendingBatches ?? 0, 10),
      )}
      <span class="pipeline-arrow">→</span>
      ${pipelineNode(
        "EvidenceStore",
        `${observations} observations`,
        `${value.evidenceStoreProvenance ?? value.observationsCreated ?? 0} provenance · ${
          formatDuration(value.evidenceFreshnessMs)
        } freshness`,
        pressureClass(observations, 25, 100),
        capacityPercent(observations, 100),
      )}
    </div>
    <div class="pipeline-meta">
      ${metric("Duplicates", value.duplicateObservations)}
      ${metric("Conflicts", value.conflictsCreated)}
      ${metric("Diagnostics", value.diagnosticsCreated)}
    </div>
  `;
}

function pipelineNode(
  label: string,
  value: string,
  detail: string,
  pressure: string,
  percent: number,
): string {
  return `
    <div class="pipeline-node ${pressure}">
      <span>${escapeHtml(label)}</span>
      <strong>${escapeHtml(value)}</strong>
      <div class="capacity-track"><i style="width: ${percent}%"></i></div>
      <small>${escapeHtml(detail)}</small>
    </div>
  `;
}

function renderDemo(snapshot: PlaygroundSnapshot): void {
  const value = snapshot.demo;
  const activeIndex = stageOrder.indexOf(value.stage);
  const running = value.stage !== "idle"
    && value.stage !== "completed"
    && value.stage !== "failed";
  demoRun.disabled = running;
  demoCancel.disabled = !running;
  demo.innerHTML = `
    <div class="demo-summary ${value.stage}">
      <strong>${escapeHtml(humanize(value.stage))}</strong>
      <span>${escapeHtml(value.detail)}</span>
    </div>
    <ol class="stage-list">
      ${stageOrder.map((stage, index) => `
        <li class="${stage === value.stage
          ? "active"
          : activeIndex > index ? "done" : ""}">
          <span>${index + 1}</span>${escapeHtml(humanize(stage))}
        </li>
      `).join("")}
    </ol>
    ${value.baseline === undefined
      ? ""
      : experienceComparison(value.baseline, value.learned)}
  `;
}

function experienceComparison(
  baseline: NonNullable<PlaygroundSnapshot["demo"]["baseline"]>,
  learned: PlaygroundSnapshot["demo"]["learned"],
): string {
  return `
    <div class="comparison">
      ${evaluationCard("Authored baseline", baseline)}
      <span class="comparison-arrow">→</span>
      ${evaluationCard("Learned executable", learned)}
    </div>
  `;
}

function evaluationCard(
  label: string,
  result: PlaygroundSnapshot["demo"]["baseline"],
): string {
  return `<div>
    <small>${escapeHtml(label)}</small>
    ${result === undefined ? "<strong>Awaiting evaluation</strong>" : `
      <strong>${(result.challengeBalance * 100).toFixed(1)}% balanced</strong>
      <small title="${(result.activePlayMs / 1_000).toFixed(1)} active seconds of ${
        result.plannedPlayMs / 1_000
      } planned">
        ${result.linesPerMinute.toFixed(1)} lines/min &middot; ${
          result.episodes - result.gameOvers
        }/${result.episodes} survived
      </small>
      <code>${escapeHtml(shortDigest(result.executableDigest))}</code>
    `}
  </div>`;
}

function renderAnalysisTimeline(values: readonly ClosedLoopEvent[]): void {
  const analysisEvents = values.filter((value): value is ClosedLoopEvent & {
    readonly phase: AnalysisPhase;
  } => isAnalysisPhase(value.phase)).sort((left, right) =>
    Date.parse(right.observedAt) - Date.parse(left.observedAt)
  );
  const visible = analysisEvents.filter((value) =>
    visibleAnalysisPhases.has(value.phase)
    && (showNotEligible || !isNotEligible(value))
  ).slice(0, 60);
  const arriving = analysisTimelineInitialized
    ? new Set(
      visible
        .filter((value) => !observedAnalysisEventIds.has(value.id))
        .map((value) => value.id),
    )
    : new Set<string>();
  analysisFilterCount.textContent =
    `${visible.length} shown · ${analysisEvents.length - visible.length} hidden`;
  timeline.innerHTML = analysisEvents.length === 0
    ? emptyState("Waiting for analysis, Candidate, and activation telemetry.")
    : visible.length === 0
      ? emptyState("All observed analysis telemetry is hidden by the filters.")
      : visible.map((value) => `
        <article
          class="timeline-event phase-${value.phase} ${
            arriving.has(value.id) ? "arriving" : ""
          }"
          data-event-id="${escapeHtml(value.id)}"
        >
          <time>${escapeHtml(formatTime(value.observedAt))}</time>
          <span class="phase">${escapeHtml(value.phase)}</span>
          <div class="timeline-event-summary">
            <strong>${escapeHtml(value.title)}</strong>
            <small>${escapeHtml(value.outcome ?? value.signal)}${
              value.durationMs === undefined
                ? ""
                : ` · ${value.durationMs.toFixed(1)}ms`
            }</small>
          </div>
          <small class="timeline-service">${escapeHtml(value.service)}</small>
          ${value.traceId === undefined
            ? `<span class="timeline-trace">—</span>`
            : `<code title="${escapeHtml(value.traceId)}">${
              escapeHtml(shortId(value.traceId))
            }</code>`}
        </article>
      `).join("");
  observedAnalysisEventIds = new Set(analysisEvents.map((value) => value.id));
  if (analysisTimelineInitialized && arriving.size > 0) {
    timeline.scrollTop = 0;
  }
  analysisTimelineInitialized = true;
}

function renderBackendFailures(snapshot: PlaygroundSnapshot): void {
  backendFailures.innerHTML = snapshot.backendFailures.length === 0
    ? ""
    : snapshot.backendFailures.map((failure) => `
      <p><strong>${escapeHtml(failure.source)}</strong>
        ${escapeHtml(failure.message)}</p>
    `).join("");
}

async function activateCandidate(executableDigest: string): Promise<void> {
  if (activationPending) return;
  activationPending = true;
  if (latest !== undefined) {
    renderExecutables(latest);
    if (executableDialog.open) renderExecutableDialog(latest);
  }
  try {
    const response = await fetch(
      `/api/executables/${encodeURIComponent(executableDigest)}/activate`,
      { method: "POST" },
    );
    const payload = await response.json() as
      | PlaygroundSnapshot
      | { readonly error?: string };
    if (!response.ok) {
      const error = "error" in payload ? payload.error : undefined;
      window.alert(error ?? `Request failed with HTTP ${response.status}.`);
      return;
    }
    latest = payload as PlaygroundSnapshot;
    selectedExecutableDigest =
      latest.executables.activeExecutable?.executableDigest;
    render(latest);
  } catch (error) {
    window.alert(error instanceof Error ? error.message : String(error));
  } finally {
    activationPending = false;
    if (latest !== undefined) {
      renderExecutables(latest);
      if (executableDialog.open) renderExecutableDialog(latest);
    }
  }
}

async function sendCommand(command: TetrisSessionCommand): Promise<void> {
  await post(`/api/commands/${command}`);
}

async function post(path: string): Promise<void> {
  const response = await fetch(path, { method: "POST" });
  if (!response.ok) {
    const payload = await response.json() as { readonly error?: string };
    window.alert(payload.error ?? `Request failed with HTTP ${response.status}.`);
  }
}

function executablePressure(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

function capacityPercent(value: number, softMaximum: number): number {
  const normalized = executablePressure(value);
  if (normalized === 0) return 4;
  return Math.max(10, Math.min(100, Math.round(
    normalized / softMaximum * 100,
  )));
}

function pressureClass(
  value: number,
  mediumThreshold: number,
  highThreshold: number,
): string {
  if (value >= highThreshold) return "pressure-high";
  if (value >= mediumThreshold) return "pressure-medium";
  return "pressure-low";
}

function isNotEligible(value: ClosedLoopEvent): boolean {
  const outcome = value.outcome
    ?? value.attributes["flaggo.operation.outcome"];
  return value.name === "flaggo.analysis.cycle.completed"
    && outcome === "not_eligible";
}

function isAnalysisPhase(value: string | undefined): value is AnalysisPhase {
  return value === "analysis" || value === "candidate" || value === "activation";
}

function formatRuleReturn(value: Readonly<Record<string, unknown>>): string {
  if (Object.hasOwn(value, "value")) {
    return JSON.stringify(value.value) ?? "null";
  }
  if (typeof value.expression === "string") return value.expression;
  return JSON.stringify(value) ?? "{}";
}

function formatBytes(value: number | undefined): string {
  if (value === undefined) return "size unavailable";
  if (value < 1_024) return `${value} B`;
  if (value < 1_048_576) return `${(value / 1_024).toFixed(1)} KiB`;
  return `${(value / 1_048_576).toFixed(1)} MiB`;
}

function formatDuration(value: number | undefined): string {
  if (value === undefined) return "unavailable";
  if (value < 1_000) return `${Math.round(value)}ms`;
  if (value < 60_000) return `${(value / 1_000).toFixed(1)}s`;
  return `${(value / 60_000).toFixed(1)}m`;
}

function healthFact(label: string, value: string): string {
  return `<div><span>${escapeHtml(label)}</span><strong>${
    escapeHtml(value)
  }</strong></div>`;
}

function metric(label: string, value: number | undefined): string {
  return `<div><span>${escapeHtml(label)}</span><strong>${
    value ?? "-"
  }</strong></div>`;
}

function kv(label: string, value: string): string {
  return `<div><span>${escapeHtml(label)}</span><code>${
    escapeHtml(value)
  }</code></div>`;
}

function emptyState(message: string): string {
  return `<p class="empty">${escapeHtml(message)}</p>`;
}

function shortServiceName(value: string): string {
  return value
    .replace(" Service", "")
    .replace("Evidence ", "")
    .replace("Async ", "");
}

function shortDigest(value: string | undefined): string {
  if (value === undefined) return "not observed";
  return value.length <= 20 ? value : `${value.slice(0, 15)}…${value.slice(-6)}`;
}

function shortId(value: string | undefined): string {
  if (value === undefined) return "not observed";
  return value.length <= 14 ? value : `${value.slice(0, 8)}…${value.slice(-4)}`;
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(value));
}

function timeAgo(value: string, now: string): string {
  const milliseconds = Math.max(0, Date.parse(now) - Date.parse(value));
  if (milliseconds < 60_000) return `${Math.round(milliseconds / 1_000)}s`;
  if (milliseconds < 3_600_000) return `${Math.round(milliseconds / 60_000)}m`;
  return `${Math.round(milliseconds / 3_600_000)}h`;
}

function formatNumber(value: number): string {
  return Number.isInteger(value)
    ? String(value)
    : value.toLocaleString(undefined, { maximumFractionDigits: 2 });
}

function humanize(value: string): string {
  const normalized = value
    .replaceAll("_", " ")
    .replaceAll("-", " ")
    .replaceAll(".", " ");
  return normalized.charAt(0).toUpperCase() + normalized.slice(1);
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function text(id: string, value: string | number): void {
  requiredElement(id).textContent = String(value);
}

function requiredElement(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (element === null) throw new Error(`Missing element '#${id}'.`);
  return element;
}

function requiredButton(id: string): HTMLButtonElement {
  const element = requiredElement(id);
  if (!(element instanceof HTMLButtonElement)) {
    throw new Error(`Element '#${id}' must be a button.`);
  }
  return element;
}

function requiredAnchor(id: string): HTMLAnchorElement {
  const element = requiredElement(id);
  if (!(element instanceof HTMLAnchorElement)) {
    throw new Error(`Element '#${id}' must be an anchor.`);
  }
  return element;
}

function requiredDialog(id: string): HTMLDialogElement {
  const element = requiredElement(id);
  if (!(element instanceof HTMLDialogElement)) {
    throw new Error(`Element '#${id}' must be a dialog.`);
  }
  return element;
}

function requiredInput(id: string): HTMLInputElement {
  const element = requiredElement(id);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Element '#${id}' must be an input.`);
  }
  return element;
}
