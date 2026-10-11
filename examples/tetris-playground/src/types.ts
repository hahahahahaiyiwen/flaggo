import type {
  TetrisSessionCommand,
  TetrisSessionState,
} from "../../tetris-integration/dist/flaggo/session.js";
import type { ExperienceTotals } from "../../tetris-integration/dist/flaggo/experience.js";

export type { TetrisSessionCommand, TetrisSessionState };
export type PlaygroundMode = "live" | "benchmark";

export interface SessionView {
  readonly state: TetrisSessionState;
  readonly decision?: DecisionView;
  readonly experience: ExperienceTotals;
  readonly error?: string;
}

export interface HumanSessionView extends SessionView {
  readonly totals: ExperienceTotals;
}

export interface BotSlotView {
  readonly id: string;
  readonly status: "idle" | "running" | "stopping" | "failed";
  readonly episode: number;
  readonly seed?: number;
  readonly session?: SessionView;
  readonly totals: ExperienceTotals;
  readonly error?: string;
}

export type ClosedLoopPhase =
  | "decision"
  | "ingestion"
  | "materialization"
  | "analysis"
  | "candidate"
  | "activation"
  | "lifecycle";

export interface ClosedLoopEvent {
  readonly id: string;
  readonly observedAt: string;
  readonly phase: ClosedLoopPhase;
  readonly service: string;
  readonly signal: "application" | "log" | "span";
  readonly name: string;
  readonly title: string;
  readonly outcome?: string;
  readonly severity?: string;
  readonly durationMs?: number;
  readonly traceId?: string;
  readonly spanId?: string;
  readonly attributes: Readonly<Record<string, string | number | boolean>>;
}

export interface ServiceStatus {
  readonly name: string;
  readonly displayName: string;
  readonly process: "running" | "exited";
  readonly readiness: "ready" | "not-ready" | "not-applicable" | "unreachable";
  readonly status: "healthy" | "degraded" | "failed" | "stopped";
  readonly detail: string;
  readonly lastSignalAt?: string;
}

export interface EvidenceProgress {
  readonly inboxRetainedBatches?: number;
  readonly inboxRetainedBytes?: number;
  readonly inboxOldestAgeMs?: number;
  readonly checkpointBatchId?: string;
  readonly pendingBatches?: number;
  readonly evidenceFreshnessMs?: number;
  readonly evidenceStoreObservations?: number;
  readonly evidenceStoreProvenance?: number;
  readonly batchesRead?: number;
  readonly observationsCreated?: number;
  readonly duplicateObservations?: number;
  readonly conflictsCreated?: number;
  readonly diagnosticsCreated?: number;
  readonly observedAt?: string;
  readonly stale: boolean;
}

export interface ControlLoopMilestone {
  readonly observedAt: string;
  readonly name: string;
  readonly outcome?: string;
  readonly status: "success" | "failure" | "waiting";
  readonly contractDigest?: string;
  readonly cycleId?: string;
  readonly candidateDigest?: string;
  readonly executableDigest?: string;
}

export interface ControlLoopProgress {
  readonly analysis?: ControlLoopMilestone;
  readonly candidate?: ControlLoopMilestone;
  readonly activation?: ControlLoopMilestone;
}

export interface MetricView {
  readonly name: string;
  readonly service?: string;
  readonly value: number;
  readonly labels: Readonly<Record<string, string>>;
}

export interface DecisionView {
  readonly observedAt: string;
  readonly sessionId: string;
  readonly attributes: Readonly<Record<string, string | number>>;
  readonly result?: number;
  readonly source: "flaggo" | "flaggo-cached" | "local" | "local-fallback";
  readonly status: string;
  readonly rule?: string;
  readonly contractDigest?: string;
  readonly executableDigest?: string;
  readonly latencyMs: number;
  readonly traceId?: string;
  readonly error?: string;
}

export interface ExecutableRuleView {
  readonly name: string;
  readonly when: {
    readonly expression: string;
  };
  readonly return: Readonly<Record<string, unknown>>;
}

export interface ManagedExecutableView {
  readonly contractDigest: string;
  readonly executableDigest: string;
  readonly lifecycleState: "active" | "candidate" | "inactive";
  readonly stateVersion: number;
  readonly createdAt: string;
  readonly activatedAt?: string;
  readonly executable: {
    readonly contractDigest: string;
    readonly kind: "rules";
    readonly rules: readonly ExecutableRuleView[];
  };
  readonly provenance?: unknown;
}

export interface ExecutableCatalogView {
  readonly status: "loading" | "ready" | "unavailable";
  readonly contractName: string;
  readonly contractDigest: string;
  readonly activeExecutable?: ManagedExecutableView;
  readonly candidates: readonly ManagedExecutableView[];
  readonly history: readonly ManagedExecutableView[];
  readonly historyCursor?: string;
  readonly nextCursor?: string;
  readonly observedAt?: string;
  readonly error?: string;
}

export type DemoStage =
  | "idle"
  | "baseline"
  | "collecting"
  | "materializing"
  | "analyzing"
  | "activating"
  | "verifying"
  | "completed"
  | "failed";

export interface DemoState {
  readonly stage: DemoStage;
  readonly detail: string;
  readonly startedAt?: string;
  readonly completedAt?: string;
  readonly baseline?: PolicyEvaluation;
  readonly learned?: PolicyEvaluation;
  readonly balanceImprovement?: number;
}

export interface PolicyEvaluation {
  readonly executableDigest: string;
  readonly episodes: number;
  readonly plannedPlayMs: number;
  readonly activePlayMs: number;
  readonly challengeBalance: number;
  readonly linesCleared: number;
  readonly linesPerMinute: number;
  readonly gameOvers: number;
}

export interface BackendFailure {
  readonly source: "loki" | "tempo" | "prometheus";
  readonly message: string;
  readonly observedAt: string;
}

export interface ActivityFeedStatus {
  readonly status: "loading" | "current" | "stale";
  readonly lastSuccessfulPollAt?: string;
  readonly error?: string;
}

export interface PlaygroundUrls {
  readonly grafana: string;
}

export interface PlaygroundSnapshot {
  readonly revision: number;
  readonly observedAt: string;
  readonly mode: PlaygroundMode;
  readonly human: HumanSessionView;
  readonly bots: readonly BotSlotView[];
  readonly executables: ExecutableCatalogView;
  readonly demo: DemoState;
  readonly services: readonly ServiceStatus[];
  readonly evidence: EvidenceProgress;
  readonly controlLoop: ControlLoopProgress;
  readonly activity: ActivityFeedStatus;
  readonly timeline: readonly ClosedLoopEvent[];
  readonly metrics: readonly MetricView[];
  readonly backendFailures: readonly BackendFailure[];
  readonly urls: PlaygroundUrls;
}

export interface ProcessState {
  readonly running: boolean;
}

export interface ReadinessState {
  readonly status: "ready" | "not-ready" | "not-applicable" | "unreachable";
  readonly body?: unknown;
}
