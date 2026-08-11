import type {
  DecisionResult,
  ExposureConfirmationResult,
  FlaggoClient,
  TelemetrySink,
} from "@flaggo/sdk";

import { createAdaptiveWorkerSignals } from "./signals.js";

export interface WorkItem {
  id: string;
  processingMs: number;
  shouldFail: boolean;
}

interface QueuedWorkItem {
  item: WorkItem;
  enqueuedAtMs: number;
}

interface PendingConfirmation {
  decisionId: string;
  confirmToken: string;
  appliedAt: string;
}

export type WorkloadProfile =
  | "steady"
  | "burst"
  | "slow-downstream"
  | "recovery";

export interface WorkerClock {
  now(): number;
  advance(milliseconds: number): void;
}

export class DeterministicWorkerClock implements WorkerClock {
  private currentMilliseconds = 0;

  now(): number {
    return this.currentMilliseconds;
  }

  advance(milliseconds: number): void {
    this.currentMilliseconds += milliseconds;
  }
}

export interface WorkerTickResult {
  profile: WorkloadProfile;
  queuePressure: number;
  queueDepthBefore: number;
  queueDepthAfter: number;
  appliedBatchSize: number;
  processedItemIds: string[];
  decision: DecisionResult<number>;
  operations: string[];
  appliedAt?: string;
  confirmation?: ExposureConfirmationResult;
}

interface ProfileDefinition {
  count: number;
  processingMs: number;
}

const profileDefinitions: Record<WorkloadProfile, ProfileDefinition> = {
  steady: { count: 2, processingMs: 10 },
  burst: { count: 8, processingMs: 10 },
  "slow-downstream": { count: 3, processingMs: 40 },
  recovery: { count: 1, processingMs: 10 },
};

export class AdaptiveWorker {
  private readonly queue: QueuedWorkItem[] = [];
  private readonly signals;
  private pendingConfirmation: PendingConfirmation | undefined;
  private tickNumber = 0;

  constructor(
    private readonly flaggo: FlaggoClient,
    telemetry: TelemetrySink,
    private readonly clock: WorkerClock = new DeterministicWorkerClock(),
    private readonly queueCapacity = 8,
    private readonly targetLatencyMs = 100,
    private readonly workerId = "adaptive-worker-1",
    private readonly claimedCohort = "worker-canary",
  ) {
    this.signals = createAdaptiveWorkerSignals(telemetry);
  }

  get queueDepth(): number {
    return this.queue.length;
  }

  get hasPendingConfirmation(): boolean {
    return this.pendingConfirmation !== undefined;
  }

  async retryPendingConfirmation():
    Promise<ExposureConfirmationResult | undefined> {
    const pending = this.pendingConfirmation;
    if (pending === undefined) return undefined;
    const confirmation = await this.flaggo.exposures.confirm(
      pending.decisionId,
      pending.confirmToken,
      { appliedAt: pending.appliedAt },
    );
    this.pendingConfirmation = undefined;
    return confirmation;
  }

  async runTick(profile: WorkloadProfile): Promise<WorkerTickResult> {
    await this.retryPendingConfirmation();
    this.enqueue(profile);
    const queueDepthBefore = this.queue.length;
    this.signals.queueDepth.emit(queueDepthBefore);
    const queuePressure = this.calculateQueuePressure();
    this.signals.queuePressure.emit(queuePressure);
    const flaggo = this.flaggo;

    const decision = await flaggo.tune.numberDetailed(
      "demo.workerBatchSize",
      {
        definition: {
          key: "demo.workerBatchSize",
          valueType: "number",
          actionSpace: {
            type: "number",
            min: 1,
            max: 10,
            step: 1,
            default: 3,
          },
          fallback: {
            value: 3,
            reason: "safe_default_worker_batch_size",
          },
          runtimeContextSchema: {
            workerId: {
              type: "string",
              required: true,
            },
            cohort: {
              type: "string",
              target: "cohort",
            },
          },
          targetHierarchy: [
            "cohort",
            "global",
          ],
          signals: {
            allowed: [
              { key: "demo.queuePressure" },
            ],
          },
          inference: {
            target: "cohort",
            inputs: [
              { key: "demo.queuePressure" },
            ],
            fallbackOrder: [
              "global",
            ],
          },
          onlineStrategy: {
            mode: "approved-strategy",
            liveInputs: [
              "queuePressure",
            ],
          },
          policy: {
            kind: "inline",
            constraints: [
              {
                kind: "number-bounds",
                min: 1,
                max: 10,
              },
              {
                kind: "max-delta",
                value: 3,
              },
              {
                kind: "cooldown",
                seconds: 1,
              },
              {
                kind: "min-evidence-quality",
                value: 0.8,
              },
            ],
            clientFallback: {
              requiredEvidenceUnavailable: "allow",
            },
          },
        },
        runtimeTarget: {
          type: "cohort",
          id: this.claimedCohort,
        },
        context: {
          workerId: this.workerId,
          cohort: this.claimedCohort,
        },
        inputs: [
          this.signals.queuePressure.input(queuePressure),
        ],
        idempotencyKey:
          `adaptive-worker:${this.workerId}:${this.tickNumber}`,
      },
    );

    const operations = [`decision-received:${decision.value}`];
    const processedItemIds = this.applyBatch(decision.value);
    operations.push(`batch-applied:${decision.value}`);
    const appliedAt = new Date().toISOString();
    let confirmation: ExposureConfirmationResult | undefined;
    if (
      decision.source === "server" &&
      decision.exposure.confirmationRequired
    ) {
      this.pendingConfirmation = {
        decisionId: decision.decisionId,
        confirmToken: decision.exposure.confirmToken,
        appliedAt,
      };
      confirmation = await this.retryPendingConfirmation();
      if (confirmation === undefined) {
        throw new Error("The pending exposure confirmation was not retained.");
      }
      operations.push(`exposure-confirmed:${confirmation.exposureId}`);
    }

    this.signals.queueDepth.emit(this.queue.length);
    return {
      profile,
      queuePressure,
      queueDepthBefore,
      queueDepthAfter: this.queue.length,
      appliedBatchSize: decision.value,
      processedItemIds,
      decision,
      operations,
      appliedAt,
      ...(confirmation === undefined ? {} : { confirmation }),
    };
  }

  async runProfiles(
    profiles: readonly WorkloadProfile[],
  ): Promise<WorkerTickResult[]> {
    const results: WorkerTickResult[] = [];
    for (const profile of profiles) {
      results.push(await this.runTick(profile));
    }
    return results;
  }

  private enqueue(profile: WorkloadProfile): void {
    const definition = profileDefinitions[profile];
    this.tickNumber += 1;
    for (let index = 0; index < definition.count; index += 1) {
      const item: WorkItem = {
        id: `${profile}-${this.tickNumber}-${index + 1}`,
        processingMs: definition.processingMs,
        shouldFail: profile === "slow-downstream" && index === 2,
      };
      this.queue.push({
        item,
        enqueuedAtMs: this.clock.now(),
      });
      this.signals.itemEnqueued.emit({
        itemId: item.id,
        processingMs: item.processingMs,
        shouldFail: item.shouldFail,
      });
    }
  }

  private calculateQueuePressure(): number {
    const oldest = this.queue[0];
    const oldestItemAgeMs = oldest === undefined
      ? 0
      : Math.max(0, this.clock.now() - oldest.enqueuedAtMs);
    return Math.min(
      1,
      Math.max(
        0,
        0.7 * this.queue.length / this.queueCapacity +
          0.3 * oldestItemAgeMs / this.targetLatencyMs,
      ),
    );
  }

  private applyBatch(batchSize: number): string[] {
    const processed: string[] = [];
    for (let index = 0; index < batchSize; index += 1) {
      const queued = this.queue.shift();
      if (queued === undefined) break;
      this.clock.advance(queued.item.processingMs);
      const processingLatencyMs =
        this.clock.now() - queued.enqueuedAtMs;
      this.signals.processingLatencyMs.emit(processingLatencyMs);
      this.signals.itemCompleted.emit({
        itemId: queued.item.id,
        succeeded: !queued.item.shouldFail,
      });
      processed.push(queued.item.id);
    }
    return processed;
  }
}
