import { randomUUID } from "node:crypto";
import { context, propagation, trace } from "@opentelemetry/api";
import {
  parseFlaggoRuntimeConfiguration,
  type FlaggoRuntimeConfiguration,
  type RuntimeContractBinding,
} from "@flaggo/sdk/configuration";
import type { FetchLike } from "@flaggo/sdk/runtime";

import type {
  DropIntervalContext,
  DropIntervalProvider,
  DropIntervalSelection,
} from "../../tetris-integration/dist/flaggo/drop-interval.js";
import { runAutonomousTetrisSession } from "../../tetris-integration/dist/flaggo/autonomous-session.js";
import {
  TetrisTelemetryProviders,
} from "../../tetris-integration/dist/flaggo/otel.js";
import {
  type TetrisSessionState,
} from "../../tetris-integration/dist/flaggo/session.js";
import { createFlaggoDropIntervalProvider } from "../../tetris-integration/dist/flaggo/flaggo-provider.js";
import {
  adaptiveDemoProtocol,
  scenarioPieceSource,
  seededRandom,
} from "../../tetris-integration/dist/flaggo/adaptive-protocol.js";
import { assertExperienceImprovement } from "./adaptive-demo.js";
import {
  createExperienceRecorder,
  TetrisExperience,
  type ExperienceTotals,
  type ExperienceWindow,
} from "../../tetris-integration/dist/flaggo/experience.js";
import {
  LgtmClient,
  type TelemetryQueryResult,
} from "./backend.js";
import {
  createObserverView,
  type ObserverView,
} from "./observer.js";
import type {
  AnalysisActionsView,
  AnalysisSelection,
  ClosedLoopEvent,
  DecisionView,
  DemoStage,
  DemoState,
  ExecutableCatalogView,
  ManagedExecutableView,
  PlaygroundSnapshot,
  PolicyEvaluation,
  ProcessState,
  ReadinessState,
  TetrisSessionCommand,
  PlaygroundMode,
} from "./types.js";
import { TetrisPopulation, type SessionProviderContext } from "./population.js";
import { PlaygroundRequestError } from "./errors.js";
import { createAnalysisActionsView, validAnalysisSelection } from "./analysis-actions.js";

type TetrisBindings = {
  readonly "tetris.dropInterval": RuntimeContractBinding;
};

interface PlaygroundConfiguration {
  readonly mode: PlaygroundMode;
  readonly demoTimeoutMilliseconds: number;
  readonly runtimeConfig: FlaggoRuntimeConfiguration<TetrisBindings>;
  readonly serviceHealthUrls: Readonly<Record<string, string>>;
  readonly serviceProcesses: Readonly<Record<string, number>>;
  readonly lgtm: {
    readonly grafanaUrl: string;
    readonly lokiUrl: string;
    readonly prometheusUrl: string;
    readonly tempoUrl: string;
  };
}

interface DecisionCapture {
  readonly attributes: Readonly<Record<string, string | number>>;
  readonly contractDigest?: string;
  readonly error?: string;
  readonly executableDigest?: string;
  readonly latencyMs: number;
  readonly rule?: string;
  readonly sessionId: string;
  readonly traceId?: string;
}

type Listener = (snapshot: PlaygroundSnapshot) => void;

const tetrisContractName = "tetris.dropInterval" as const;

export function loadPlaygroundConfiguration(
  environment: NodeJS.ProcessEnv = process.env,
): PlaygroundConfiguration {
  const mode = requiredEnvironment(environment, "FLAGGO_PLAYGROUND_MODE");
  if (mode !== "live" && mode !== "benchmark") {
    throw new Error("FLAGGO_PLAYGROUND_MODE must be 'live' or 'benchmark'.");
  }
  return {
    mode,
    demoTimeoutMilliseconds: positiveIntegerEnvironment(
      environment,
      "FLAGGO_PLAYGROUND_DEMO_TIMEOUT_MS",
      45_000,
    ),
    runtimeConfig: parseFlaggoRuntimeConfiguration<TetrisBindings>(
      parseEnvironmentJson(environment, "FLAGGO_PLAYGROUND_RUNTIME_CONFIG"),
    ),
    serviceHealthUrls: stringMap(
      parseEnvironmentJson(environment, "FLAGGO_PLAYGROUND_HEALTH_URLS"),
      "FLAGGO_PLAYGROUND_HEALTH_URLS",
    ),
    serviceProcesses: numberMap(
      parseEnvironmentJson(environment, "FLAGGO_PLAYGROUND_SERVICE_PROCESSES"),
      "FLAGGO_PLAYGROUND_SERVICE_PROCESSES",
    ),
    lgtm: {
      grafanaUrl: requiredEnvironment(
        environment,
        "FLAGGO_PLAYGROUND_GRAFANA_URL",
      ),
      lokiUrl: requiredEnvironment(environment, "FLAGGO_PLAYGROUND_LOKI_URL"),
      prometheusUrl: requiredEnvironment(
        environment,
        "FLAGGO_PLAYGROUND_PROMETHEUS_URL",
      ),
      tempoUrl: requiredEnvironment(environment, "FLAGGO_PLAYGROUND_TEMPO_URL"),
    },
  };
}

export class TetrisPlayground {
  private readonly telemetry: TetrisTelemetryProviders;
  private readonly population: TetrisPopulation;
  private readonly mode: PlaygroundMode;
  private readonly recordExperience: (window: ExperienceWindow) => void;
  private readonly runtimeConfig: PlaygroundConfiguration["runtimeConfig"];
  private readonly captures = new Map<string, DecisionCapture>();
  private readonly fetchImpl: FetchLike;
  private readonly lgtm: LgtmClient;
  private readonly listeners = new Set<Listener>();
  private readonly applicationEvents: ClosedLoopEvent[] = [];
  private readonly startedAt = new Date();
  private readonly healthUrls: Readonly<Record<string, string>>;
  private readonly serviceProcesses: Readonly<Record<string, number>>;
  private readonly grafanaUrl: string;
  private readonly demoTimeoutMilliseconds: number;
  private readonly contractServiceUrl: string;
  private readonly contractDigest: string;
  private telemetryResult: TelemetryQueryResult = {
    logs: [],
    spans: [],
    metrics: [],
    failures: [],
  };
  private executables: ExecutableCatalogView;
  private executableHistoryCursor: string | undefined;
  private demo: DemoState = {
    stage: "idle",
    detail: "Compare sustained baseline and learned gameplay on matched held-out games.",
  };
  private observer: ObserverView;
  private revision = 0;
  private closed = false;
  private polling = false;
  private demoController: AbortController | undefined;
  private demoTask: Promise<void> | undefined;
  private observerTimer: NodeJS.Timeout | undefined;

  constructor(configuration: PlaygroundConfiguration) {
    this.mode = configuration.mode;
    this.healthUrls = configuration.serviceHealthUrls;
    this.serviceProcesses = configuration.serviceProcesses;
    this.grafanaUrl = configuration.lgtm.grafanaUrl;
    this.demoTimeoutMilliseconds = configuration.demoTimeoutMilliseconds;
    this.contractServiceUrl =
      configuration.runtimeConfig.services.contractServiceUrl.replace(/\/+$/u, "");
    this.contractDigest =
      configuration.runtimeConfig.bindings[tetrisContractName].contractDigest;
    this.executables = {
      status: "loading",
      contractName: tetrisContractName,
      contractDigest: this.contractDigest,
      candidates: [],
      history: [],
    };
    this.lgtm = new LgtmClient(configuration.lgtm);
    this.observer = createObserverView({
      telemetry: this.telemetryResult,
      processes: processStates(this.serviceProcesses),
      readiness: {},
      now: new Date(),
    });
    this.observer = {
      ...this.observer,
      activity: { status: "loading" },
      analysisActions: { ...this.observer.analysisActions, status: "loading" },
    };
    this.telemetry = new TetrisTelemetryProviders({
      runtimeConfig: configuration.runtimeConfig,
    });
    this.runtimeConfig = configuration.runtimeConfig;
    this.fetchImpl = capturingFetch(fetch, this.captures);
    this.recordExperience = createExperienceRecorder(this.telemetry.getMeter("tetris.experience"));
    this.population = new TetrisPopulation({
      mode: this.mode,
      createProvider: (options) => this.createProvider(options),
      recordExperience: this.recordExperience,
      onChanged: () => this.changed(),
      instrumentation: this.telemetry.sessionInstrumentation,
    });
  }

  start(): void {
    this.population.start();
    this.observerTimer = setInterval(() => {
      void this.pollObserver();
    }, 1_000);
    void this.pollObserver();
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    listener(this.snapshot());
    return () => this.listeners.delete(listener);
  }

  snapshot(): PlaygroundSnapshot {
    return {
      revision: this.revision,
      observedAt: new Date().toISOString(),
      mode: this.mode,
      ...this.population.snapshot(),
      analysisActions: this.observer.analysisActions,
      executables: this.executables,
      demo: this.demo,
      services: this.observer.services,
      evidence: this.observer.evidence,
      controlLoop: this.observer.controlLoop,
      activity: this.observer.activity,
      timeline: this.observer.timeline,
      metrics: this.observer.metrics,
      backendFailures: this.telemetryResult.failures,
      urls: { grafana: this.grafanaUrl },
    };
  }

  async inspectAnalysis(selection: AnalysisSelection): Promise<AnalysisActionsView> {
    if (!validAnalysisSelection(selection)) {
      throw new PlaygroundRequestError(400, "Invalid analysis telemetry selection.");
    }
    let logs: TelemetryQueryResult["logs"] = [];
    let queryLimited = false;
    let error: string | undefined;
    try {
      const result = await this.lgtm.queryAnalysisActions(this.startedAt, selection);
      logs = result.logs;
      queryLimited = result.limited;
    } catch {
      error = "Loki action telemetry unavailable; retaining latest observed facts.";
    }
    const now = new Date();
    const inspected = createAnalysisActionsView({
      logs, now, previous: this.observer.analysisActions, selection,
      queryLimited, ...(error === undefined ? {} : { error }),
    });
    const analysisActions = createAnalysisActionsView({
      logs, now, previous: this.observer.analysisActions,
      queryLimited, ...(error === undefined ? {} : { error }),
    });
    this.observer = { ...this.observer, analysisActions };
    this.changed();
    return inspected;
  }

  command(command: TetrisSessionCommand): PlaygroundSnapshot {
    this.population.command(command);
    return this.snapshot();
  }

  startBot(id: string): PlaygroundSnapshot {
    this.population.startBot(id);
    return this.snapshot();
  }

  startAllBots(): PlaygroundSnapshot {
    if (this.mode !== "live") throw new PlaygroundRequestError(409, "Bots require live mode.");
    this.population.startAll();
    return this.snapshot();
  }

  async stopBot(id: string): Promise<PlaygroundSnapshot> {
    await this.population.stopBot(id);
    return this.snapshot();
  }

  async stopAllBots(): Promise<PlaygroundSnapshot> {
    await this.population.stopAll();
    return this.snapshot();
  }

  runDemo(): DemoState {
    if (this.mode !== "benchmark") {
      throw new PlaygroundRequestError(409, "Run the guided comparison in --mode=benchmark.");
    }
    if (this.demoController !== undefined) {
      throw new Error("The closed-loop demo is already running.");
    }
    const controller = new AbortController();
    this.demoController = controller;
    const startedAt = new Date().toISOString();
    this.demo = {
      stage: "baseline",
      detail: "Measuring the authored baseline on three held-out mixed-piece games.",
      startedAt,
    };
    this.changed();
    const task = this.executeDemo(startedAt, controller.signal).finally(() => {
      if (this.demoController === controller) this.demoController = undefined;
      if (this.demoTask === task) this.demoTask = undefined;
    });
    this.demoTask = task;
    return this.demo;
  }

  cancelDemo(): DemoState {
    if (this.demoController === undefined) {
      throw new Error("No closed-loop demo is running.");
    }
    this.demoController.abort(new Error("Demo cancelled by the user."));
    return this.demo;
  }

  async activateExecutable(executableDigest: string): Promise<PlaygroundSnapshot> {
    const candidate = this.executables.history.find(
      (value) => value.executableDigest === executableDigest
        && value.lifecycleState === "candidate",
    );
    const active = this.executables.activeExecutable;
    if (
      this.executables.status !== "ready"
      || candidate === undefined
      || active === undefined
    ) {
      throw new PlaygroundRequestError(
        409,
        "The selected Candidate is no longer available. Refresh and select it again.",
      );
    }

    const response = await fetch(
      `${this.executableCatalogUrl()}/${encodeURIComponent(executableDigest)}`
        + "/activate",
      {
        method: "POST",
        headers: {
          Accept: "application/json",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          expectedActiveExecutableDigest: active.executableDigest,
          expectedActiveStateVersion: active.stateVersion,
          expectedCandidateStateVersion: candidate.stateVersion,
        }),
        signal: AbortSignal.timeout(2_000),
      },
    );
    const payload = await parseResponse(response);
    if (!response.ok) {
      this.executables = await this.observeExecutableCatalog();
      this.changed();
      throw new PlaygroundRequestError(
        response.status,
        problemMessage(payload, response),
      );
    }

    this.executables = await this.observeExecutableCatalog();
    await this.population.refreshHumanPolicy();
    this.changed();
    return this.snapshot();
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.demoController?.abort(new Error("Playground shutting down."));
    if (this.observerTimer !== undefined) clearInterval(this.observerTimer);
    await this.demoTask;
    await this.population.close();
    try {
      await this.telemetry.forceFlush();
    } finally {
      await this.telemetry.shutdown();
    }
  }

  async showExecutableHistory(cursor?: string): Promise<PlaygroundSnapshot> {
    const catalog = await this.fetchExecutableCatalog(cursor);
    this.executableHistoryCursor = cursor;
    this.executables = catalog;
    this.changed();
    return this.snapshot();
  }

  private async executeDemo(
    startedAt: string,
    signal: AbortSignal,
  ): Promise<void> {
    try {
      const catalog = await this.fetchExecutableCatalog(this.executableHistoryCursor);
      const authored = catalog.activeExecutable;
      if (authored === undefined
        || !authored.executable.rules.some((rule) => rule.name === "explore-fast-gravity")) {
        throw new Error("Start a fresh playground to compare the authored exploratory baseline.");
      }
      const baseline = await this.evaluatePolicy(authored.executableDigest, signal);
      this.setDemo(
        "collecting",
        "Collecting nine independently paced training games; held-out results stay out of learning.",
        { baseline },
      );
      for (const [index, seed] of adaptiveDemoProtocol.trainingSeeds.entries()) {
        signal.throwIfAborted();
        await this.runEpisode(seed, true, authored.executableDigest, signal);
        this.setDemo(
          "collecting",
          `${index + 1}/9 training games completed through real gravity, placement, and line clearing.`,
        );
      }
      await this.telemetry.forceFlush();
      this.setDemo(
        "materializing",
        "Telemetry flushed; waiting for evidence materialization.",
      );

      const deadline = Date.now() + this.demoTimeoutMilliseconds;
      while (Date.now() < deadline) {
        signal.throwIfAborted();
        await delay(750, signal);
        await this.pollObserver();
        this.updateDemoStageFromTimeline(startedAt);
        const active = this.executables.activeExecutable;
        if (active !== undefined && active.executableDigest !== baseline.executableDigest) {
          this.setDemo("verifying", "Measuring the activated policy on the same held-out games.");
          const learned = await this.evaluatePolicy(active.executableDigest, signal);
          this.setDemo("verifying", "Checking measured balance, throughput, and survival.", { learned });
          assertExperienceImprovement(baseline, learned);
          const balanceImprovement = learned.challengeBalance - baseline.challengeBalance;
          this.setDemo(
            "completed",
            `Held-out balance improved ${(balanceImprovement * 100).toFixed(1)} percentage points; `
              + "throughput and survival guardrails passed. This is model evidence, not a human-fun claim.",
            {
              balanceImprovement,
              completedAt: new Date().toISOString(),
            },
          );
          await this.pollObserver();
          return;
        }
      }
      throw new Error(
        "Timed out waiting for a distinct evidence-supported activated executable.",
      );
    } catch (error) {
      this.setDemo("failed", errorMessage(error), {
        completedAt: new Date().toISOString(),
      });
    }
  }

  private async evaluatePolicy(
    executableDigest: string,
    signal: AbortSignal,
  ): Promise<PolicyEvaluation> {
    const totals: ExperienceTotals[] = [];
    for (const seed of adaptiveDemoProtocol.evaluationSeeds) {
      totals.push(await this.runEpisode(seed, false, executableDigest, signal));
    }
    const sum = (name: keyof ExperienceTotals): number =>
      totals.reduce((value, total) => value + total[name], 0);
    const activePlayMs = sum("activePlayMs");
    if (activePlayMs === 0) throw new Error("Evaluation produced no active gameplay.");
    return {
      executableDigest,
      episodes: totals.length,
      plannedPlayMs: adaptiveDemoProtocol.durationMs * totals.length,
      activePlayMs,
      challengeBalance: sum("balancedPlayMs") / activePlayMs,
      linesCleared: sum("linesCleared"),
      linesPerMinute: sum("linesCleared") * 60_000 / activePlayMs,
      gameOvers: sum("gameOvers"),
    };
  }

  private async runEpisode(
    seed: number,
    learning: boolean,
    executableDigest: string,
    signal: AbortSignal,
  ): Promise<ExperienceTotals> {
    const experience = new TetrisExperience(
      { dataset: learning ? "training" : "evaluation", origin: "greedy", clock: "virtual" },
      learning ? this.recordExperience : undefined,
    );
    let state: TetrisSessionState | undefined;
    let clock = 0;
    let decisionFailure: Error | undefined;
    const provider = this.createProvider({
      experience,
      state: () => {
        if (state === undefined) throw new Error("Observe the autonomous session before deciding.");
        return state;
      },
      now: () => clock,
      random: seededRandom(seed ^ 0x5F3759DF),
      learning,
      onDecision: (decision) => {
        if (decision.source !== "flaggo"
          || decision.contractDigest !== this.contractDigest
          || decision.executableDigest !== executableDigest) {
          decisionFailure = new Error(
            decision.error ?? "Executable authority changed or became unobservable during gameplay.",
          );
        }
      },
    });
    const result = await runAutonomousTetrisSession({
      durationMs: adaptiveDemoProtocol.durationMs,
      reactionDelayMs: adaptiveDemoProtocol.reactionDelayMs,
      commandIntervalMs: adaptiveDemoProtocol.commandIntervalMs,
      sessionId: `tetris-demo-${randomUUID()}`,
      pieceSource: scenarioPieceSource(seed),
      provider,
      signal,
      ...(learning ? { instrumentation: this.telemetry.sessionInstrumentation } : {}),
      onState: (value, now, transition) => {
        state = value;
        clock = now;
        experience.observe(value, now, transition);
        if (value.status !== "closed" && decisionFailure !== undefined) throw decisionFailure;
      },
    });
    const totals = experience.totals();
    if (totals.linesCleared !== result.state.lines || totals.activePlayMs !== result.elapsedMs) {
      throw new Error("Projected experience does not match actual episode outcomes.");
    }
    experience.completeEpisode();
    return totals;
  }

  private createProvider(options: SessionProviderContext): DropIntervalProvider {
    const provider = createFlaggoDropIntervalProvider({
      ...options,
      runtimeConfig: this.runtimeConfig,
      fetch: this.fetchImpl,
      ...(options.learning ? { telemetry: this.telemetry.policyInstrumentation } : {}),
    });
    return observedProvider(provider, this.captures, (decision) => {
      options.onDecision?.(decision);
      this.applicationEvents.unshift(decisionEvent(decision));
      if (this.applicationEvents.length > 100) this.applicationEvents.pop();
      this.changed();
    });
  }

  private updateDemoStageFromTimeline(startedAt: string): void {
    const current = this.demo.stage;
    if (current === "completed" || current === "failed") return;
    const recent = this.observer.timeline.filter(
      (event) => event.observedAt >= startedAt,
    );
    if (recent.some((event) => event.phase === "activation")) {
      this.setDemo("verifying", "Activation observed; verifying behavior.");
    } else if (recent.some((event) => event.phase === "candidate")) {
      this.setDemo("activating", "Candidate admitted; waiting for activation.");
    } else if (recent.some((event) => event.phase === "analysis")) {
      this.setDemo("analyzing", "Evidence available; analysis is running.");
    } else if (recent.some((event) => event.phase === "materialization")) {
      this.setDemo("analyzing", "Evidence materialized; waiting for analysis.");
    }
  }

  private setDemo(
    stage: DemoStage,
    detail: string,
    update: Partial<DemoState> = {},
  ): void {
    this.demo = {
      ...this.demo,
      ...update,
      stage,
      detail,
    };
    this.changed();
  }

  private async pollObserver(): Promise<void> {
    if (this.polling || this.closed) return;
    this.polling = true;
    try {
      const historyCursor = this.executableHistoryCursor;
      const [telemetryResult, readiness, executables] = await Promise.all([
        this.lgtm.query(this.startedAt),
        pollReadiness(this.healthUrls),
        this.observeExecutableCatalog(),
      ]);
      this.telemetryResult = telemetryResult;
      if (historyCursor === this.executableHistoryCursor) this.executables = executables;
      this.observer = createObserverView({
        telemetry: telemetryResult,
        processes: processStates(this.serviceProcesses),
        readiness,
        now: new Date(),
        applicationEvents: this.applicationEvents,
        previous: this.observer,
      });
      this.changed();
    } finally {
      this.polling = false;
    }
  }

  private async observeExecutableCatalog(): Promise<ExecutableCatalogView> {
    try {
      return await this.fetchExecutableCatalog(this.executableHistoryCursor);
    } catch (error) {
      return {
        ...this.executables,
        status: "unavailable",
        error: errorMessage(error),
      };
    }
  }

  private async fetchExecutableCatalog(
    cursor: string | undefined,
  ): Promise<ExecutableCatalogView> {
    const historyUrl = new URL(`${this.executableCatalogUrl()}/history`);
    if (cursor !== undefined) historyUrl.searchParams.set("cursor", cursor);
    const [catalogPayload, historyPayload] = await Promise.all([
      this.fetchManagementJson(this.executableCatalogUrl()),
      this.fetchManagementJson(historyUrl.toString()),
    ]);
    const catalog = parseExecutableCatalog(catalogPayload);
    const history = requiredObject(historyPayload, "executable history");
    if (
      requiredString(history, "contractName") !== catalog.contractName
      || requiredString(history, "contractDigest") !== catalog.contractDigest
    ) {
      throw new Error("Executable history does not match the authority catalog.");
    }
    if (!Array.isArray(history.executables)) {
      throw new Error("Executable history executables must be an array.");
    }
    const nextCursor = history.nextCursor;
    if (nextCursor !== null && typeof nextCursor !== "string") {
      throw new Error("Executable history nextCursor must be a string or null.");
    }
    return {
      ...catalog,
      history: history.executables.map((value, index) =>
        parseManagedExecutable(value, `History executable ${index}`)
      ),
      ...(cursor === undefined ? {} : { historyCursor: cursor }),
      ...(typeof nextCursor === "string" ? { nextCursor } : {}),
      status: "ready",
      observedAt: new Date().toISOString(),
    };
  }

  private async fetchManagementJson(url: string): Promise<unknown> {
    const response = await fetch(url, {
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(5_000),
    });
    const payload = await parseResponse(response);
    if (!response.ok) {
      throw new PlaygroundRequestError(
        response.status,
        problemMessage(payload, response),
      );
    }
    return payload;
  }

  private executableCatalogUrl(): string {
    return `${this.contractServiceUrl}/v3/decision-contracts/`
      + `${encodeURIComponent(tetrisContractName)}/versions/`
      + `${encodeURIComponent(this.contractDigest)}/executables`;
  }

  private changed(): void {
    this.revision += 1;
    const snapshot = this.snapshot();
    for (const listener of this.listeners) listener(snapshot);
  }
}

function observedProvider(
  provider: DropIntervalProvider,
  captures: Map<string, DecisionCapture>,
  onDecision: (decision: DecisionView) => void,
): DropIntervalProvider {
  return {
    async select(
      decisionContext: DropIntervalContext,
      signal?: AbortSignal,
    ): Promise<DropIntervalSelection> {
      const startedAt = performance.now();
      try {
        const selection = await provider.select(decisionContext, signal);
        const capture = captures.get(decisionContext.sessionId);
        onDecision({
          observedAt: new Date().toISOString(),
          sessionId: decisionContext.sessionId,
          attributes: capture?.attributes ?? {},
          result: selection.intervalMs,
          source: selection.source,
          status: selection.status,
          ...(capture?.rule === undefined ? {} : { rule: capture.rule }),
          ...(capture?.contractDigest === undefined
            ? {}
            : { contractDigest: capture.contractDigest }),
          ...(capture?.executableDigest === undefined
            ? {}
            : { executableDigest: capture.executableDigest }),
          latencyMs: capture?.latencyMs ?? performance.now() - startedAt,
          ...(capture?.traceId === undefined ? {} : { traceId: capture.traceId }),
          ...(selection.source !== "local-fallback"
            ? {}
            : { error: capture?.error ?? selection.status }),
        });
        return selection;
      } finally {
        captures.delete(decisionContext.sessionId);
      }
    },
  };
}

function capturingFetch(
  baseFetch: typeof fetch,
  captures: Map<string, DecisionCapture>,
): FetchLike {
  return async (input, init) => {
    const body = parseRequestBody(init?.body);
    const sessionId = stringProperty(
      objectValue(body?.attributes),
      "session_id",
    );
    const attributes = sessionId === undefined ? {} : decisionInputAttributes(body?.attributes);
    const activeSpan = trace.getSpan(context.active());
    const traceId = activeSpan?.spanContext().traceId;
    const headers = new Headers(init?.headers);
    const carrier: Record<string, string> = {};
    propagation.inject(context.active(), carrier);
    for (const [name, value] of Object.entries(carrier)) {
      headers.set(name, value);
    }
    if (activeSpan !== undefined) {
      const spanContext = activeSpan.spanContext();
      const flags = spanContext.traceFlags.toString(16).padStart(2, "0");
      headers.set(
        "traceparent",
        `00-${spanContext.traceId}-${spanContext.spanId}-${flags}`,
      );
    }
    const startedAt = performance.now();
    try {
      const response = await baseFetch(input, {
        ...init,
        headers,
      });
      if (sessionId !== undefined) {
        const payload = await parseResponse(response.clone());
        captures.set(sessionId, {
          sessionId,
          attributes,
          latencyMs: performance.now() - startedAt,
          ...(traceId === undefined ? {} : { traceId }),
          ...responseCapture(payload, response),
        });
      }
      return response;
    } catch (error) {
      if (sessionId !== undefined) {
        captures.set(sessionId, {
          sessionId,
          attributes,
          latencyMs: performance.now() - startedAt,
          ...(traceId === undefined ? {} : { traceId }),
          error: errorMessage(error),
        });
      }
      throw error;
    }
  };
}

async function parseResponse(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    return undefined;
  }
}

function responseCapture(
  value: unknown,
  response: Response,
): Omit<DecisionCapture, "sessionId" | "latencyMs" | "traceId" | "attributes"> {
  const payload = objectValue(value);
  const evaluation = objectValue(payload?.evaluation);
  return {
    ...optionalProperty(payload, "contractDigest", "contractDigest"),
    ...optionalProperty(payload, "executableDigest", "executableDigest"),
    ...optionalProperty(evaluation, "rule", "rule"),
    ...(response.ok
      ? {}
      : {
        error: `${response.status} ${response.statusText}`
          + (stringProperty(payload, "detail") === undefined
            ? ""
            : `: ${stringProperty(payload, "detail")}`),
      }),
  };
}

function decisionInputAttributes(value: unknown): Readonly<Record<string, string | number>> {
  const input = requiredObject(value, "decision attributes");
  const result: Record<string, string | number> = {};
  for (const [name, attribute] of Object.entries(input)) {
    if (typeof attribute !== "string" && typeof attribute !== "number") {
      throw new Error(`Decision attribute '${name}' must be a string or number.`);
    }
    result[name] = attribute;
  }
  return result;
}

function decisionEvent(decision: DecisionView): ClosedLoopEvent {
  return {
    id: `application:${decision.sessionId}:${decision.observedAt}`,
    observedAt: decision.observedAt,
    phase: "decision",
    service: "tetris",
    signal: "application",
    name: "flaggo.decision.received",
    title: `${decision.sessionId}: ${decision.result ?? "no result"}ms`,
    outcome: decision.source === "local-fallback" ? "fallback" : "success",
    durationMs: decision.latencyMs,
    ...(decision.traceId === undefined ? {} : { traceId: decision.traceId }),
    attributes: {
      ...decision.attributes,
      "tetris.session.id": decision.sessionId,
      ...(decision.result === undefined
        ? {}
        : { "flaggo.result": decision.result }),
      ...(decision.contractDigest === undefined
        ? {}
        : { "flaggo.contract.digest": decision.contractDigest }),
      ...(decision.executableDigest === undefined
        ? {}
        : { "flaggo.executable.digest": decision.executableDigest }),
    },
  };
}

async function pollReadiness(
  urls: Readonly<Record<string, string>>,
): Promise<Readonly<Record<string, ReadinessState>>> {
  const entries = await Promise.all(Object.entries(urls).map(
    async ([service, url]): Promise<readonly [string, ReadinessState]> => {
      const signal = AbortSignal.timeout(800);
      try {
        const response = await fetch(url, {
          headers: { Accept: "application/json" },
          signal,
        });
        const body = await parseResponse(response);
        return [service, {
          status: response.ok ? "ready" : "not-ready",
          body,
        }];
      } catch {
        return [service, { status: "unreachable" }];
      }
    },
  ));
  return Object.fromEntries(entries);
}

function processStates(
  processes: Readonly<Record<string, number>>,
): Readonly<Record<string, ProcessState>> {
  return Object.fromEntries(Object.entries(processes).map(([service, pid]) => {
    try {
      process.kill(pid, 0);
      return [service, { running: true }];
    } catch {
      return [service, { running: false }];
    }
  }));
}

function parseEnvironmentJson(
  environment: NodeJS.ProcessEnv,
  name: string,
): unknown {
  const value = requiredEnvironment(environment, name);
  try {
    return JSON.parse(value);
  } catch (error) {
    throw new Error(`${name} must contain valid JSON.`, { cause: error });
  }
}

function requiredEnvironment(
  environment: NodeJS.ProcessEnv,
  name: string,
): string {
  const value = environment[name];
  if (value === undefined || value.trim() === "") {
    throw new Error(`${name} is required.`);
  }
  return value;
}

function positiveIntegerEnvironment(
  environment: NodeJS.ProcessEnv,
  name: string,
  defaultValue: number,
): number {
  const value = environment[name];
  if (value === undefined || value.trim() === "") return defaultValue;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a positive integer.`);
  }
  return parsed;
}

function stringMap(value: unknown, name: string): Record<string, string> {
  const input = objectValue(value);
  if (input === undefined) throw new Error(`${name} must contain an object.`);
  const result: Record<string, string> = {};
  for (const [key, item] of Object.entries(input)) {
    if (typeof item !== "string" || item === "") {
      throw new Error(`${name}.${key} must be a non-empty string.`);
    }
    result[key] = item;
  }
  return result;
}

function numberMap(value: unknown, name: string): Record<string, number> {
  const input = objectValue(value);
  if (input === undefined) throw new Error(`${name} must contain an object.`);
  const result: Record<string, number> = {};
  for (const [key, item] of Object.entries(input)) {
    if (typeof item !== "number" || !Number.isInteger(item) || item <= 0) {
      throw new Error(`${name}.${key} must be a positive process ID.`);
    }
    result[key] = item;
  }
  return result;
}

function parseRequestBody(body: BodyInit | null | undefined):
  | Record<string, unknown>
  | undefined {
  if (typeof body !== "string") return undefined;
  try {
    return objectValue(JSON.parse(body));
  } catch {
    return undefined;
  }
}

function parseExecutableCatalog(
  value: unknown,
): Omit<ExecutableCatalogView, "history" | "historyCursor" | "nextCursor"> {
  const payload = requiredObject(value, "executable catalog");
  const candidates = payload.candidates;
  if (!Array.isArray(candidates)) {
    throw new Error("Executable catalog candidates must be an array.");
  }
  const nextCursor = payload.nextCursor;
  if (nextCursor !== null && nextCursor !== undefined && typeof nextCursor !== "string") {
    throw new Error("Executable catalog nextCursor must be a string or null.");
  }
  return {
    status: "ready",
    contractName: requiredString(payload, "contractName"),
    contractDigest: requiredString(payload, "contractDigest"),
    activeExecutable: parseManagedExecutable(
      payload.activeExecutable,
      "active executable",
    ),
    candidates: candidates.map((candidate, index) =>
      parseManagedExecutable(candidate, `Candidate ${index}`)
    ),
  };
}

function parseManagedExecutable(
  value: unknown,
  label: string,
): ManagedExecutableView {
  const payload = requiredObject(value, label);
  const lifecycleState = requiredString(payload, "lifecycleState");
  if (
    lifecycleState !== "active"
    && lifecycleState !== "candidate"
    && lifecycleState !== "inactive"
  ) {
    throw new Error(`${label} lifecycleState is invalid.`);
  }
  const executable = requiredObject(payload.executable, `${label} executable`);
  if (requiredString(executable, "kind") !== "rules") {
    throw new Error(`${label} executable kind must be rules.`);
  }
  const rules = executable.rules;
  if (!Array.isArray(rules)) {
    throw new Error(`${label} executable rules must be an array.`);
  }
  return {
    contractDigest: requiredString(payload, "contractDigest"),
    executableDigest: requiredString(payload, "executableDigest"),
    lifecycleState,
    stateVersion: requiredInteger(payload, "stateVersion"),
    createdAt: requiredString(payload, "createdAt"),
    ...optionalString(payload, "activatedAt"),
    executable: {
      contractDigest: requiredString(executable, "contractDigest"),
      kind: "rules",
      rules: rules.map((ruleValue, index) => {
        const rule = requiredObject(ruleValue, `${label} rule ${index}`);
        const when = requiredObject(rule.when, `${label} rule ${index} when`);
        return {
          name: requiredString(rule, "name"),
          when: {
            expression: requiredString(when, "expression"),
          },
          return: requiredObject(
            rule.return,
            `${label} rule ${index} return`,
          ),
        };
      }),
    },
    ...(Object.hasOwn(payload, "provenance")
      ? { provenance: payload.provenance }
      : {}),
  };
}

function requiredObject(
  value: unknown,
  label: string,
): Record<string, unknown> {
  const result = objectValue(value);
  if (result === undefined) throw new Error(`${label} must be an object.`);
  return result;
}

function requiredString(
  value: Record<string, unknown>,
  name: string,
): string {
  const result = value[name];
  if (typeof result !== "string" || result === "") {
    throw new Error(`${name} must be a non-empty string.`);
  }
  return result;
}

function requiredInteger(
  value: Record<string, unknown>,
  name: string,
): number {
  const result = value[name];
  if (
    typeof result !== "number"
    || !Number.isSafeInteger(result)
    || result < 0
  ) {
    throw new Error(`${name} must be a non-negative integer.`);
  }
  return result;
}

function optionalString<TKey extends string>(
  value: Record<string, unknown>,
  name: TKey,
): { readonly [key in TKey]?: string } {
  const result = value[name];
  if (result === undefined || result === null) return {};
  if (typeof result !== "string" || result === "") {
    throw new Error(`${name} must be a non-empty string when present.`);
  }
  return { [name]: result } as { readonly [key in TKey]?: string };
}

function problemMessage(value: unknown, response: Response): string {
  const payload = objectValue(value);
  return stringProperty(payload, "detail")
    ?? stringProperty(payload, "title")
    ?? `Request failed with HTTP ${response.status}.`;
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function stringProperty(
  value: Record<string, unknown> | undefined,
  name: string,
): string | undefined {
  const item = value?.[name];
  return typeof item === "string" && item !== "" ? item : undefined;
}

function optionalProperty<TKey extends string>(
  value: Record<string, unknown> | undefined,
  source: string,
  target: TKey,
): { readonly [key in TKey]?: string } {
  const item = stringProperty(value, source);
  return item === undefined
    ? {}
    : { [target]: item } as { readonly [key in TKey]?: string };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function delay(milliseconds: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, milliseconds);
    const onAbort = (): void => {
      clearTimeout(timeout);
      reject(signal.reason);
    };
    signal.addEventListener("abort", onAbort, { once: true });
    if (signal.aborted) onAbort();
  });
}
