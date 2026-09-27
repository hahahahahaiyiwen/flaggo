import {
  type DecisionClient,
  type DecisionSpec,
  type RuntimeDecision,
} from "@flaggo/sdk/runtime";
import type {
  ApplicationLogger,
} from "./telemetry.js";
export interface WorkItem {
  id: string;
  processingMs: number;
  shouldFail: boolean;
}

interface QueuedWorkItem {
  item: WorkItem;
  enqueuedAtMs: number;
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
  decision: RuntimeDecision<number>;
  operations: string[];
  appliedAt?: string;
}

export type WorkerDecisions = {
  readonly "demo.workerBatchSize": DecisionSpec<{
    readonly workerId: string;
    readonly cohort: string;
    readonly queuePressure: number;
  }, number>;
};

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
  private tickNumber = 0;

  constructor(
    private readonly flaggo: DecisionClient<WorkerDecisions>,
    private readonly telemetry: ApplicationLogger,
    private readonly clock: WorkerClock = new DeterministicWorkerClock(),
    private readonly queueCapacity = 8,
    private readonly targetLatencyMs = 100,
    private readonly workerId = "adaptive-worker-1",
    private readonly claimedCohort = "worker-canary",
  ) {}

  get queueDepth(): number {
    return this.queue.length;
  }

  async runTick(profile: WorkloadProfile): Promise<WorkerTickResult> {
    this.enqueue(profile);
    const queueDepthBefore = this.queue.length;
    this.telemetry.emit({ eventName: "worker.queue.depth", body: queueDepthBefore });
    const queuePressure = this.calculateQueuePressure();
    this.telemetry.emit({ eventName: "worker.queue.pressure", body: queuePressure });
    const flaggo = this.flaggo;

    const { value: decision } = await flaggo.decide(
      "demo.workerBatchSize",
      {
        attributes: {
          workerId: this.workerId,
          cohort: this.claimedCohort,
          queuePressure,
        },
      },
    );

    const operations = [`decision-received:${decision.result}`];
    const processedItemIds = this.applyBatch(decision.result);
    operations.push(`batch-applied:${decision.result}`);
    const appliedAt = new Date().toISOString();
    this.telemetry.emit({
      eventName: "worker.batch.applied",
      body: { batchSize: decision.result, processedCount: processedItemIds.length },
    });

    this.telemetry.emit({ eventName: "worker.queue.depth", body: this.queue.length });
    return {
      profile,
      queuePressure,
      queueDepthBefore,
      queueDepthAfter: this.queue.length,
      appliedBatchSize: decision.result,
      processedItemIds,
      decision,
      operations,
      appliedAt,
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
      this.telemetry.emit({
        eventName: "worker.item.enqueued",
        body: { itemId: item.id, processingMs: item.processingMs, shouldFail: item.shouldFail },
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
      this.telemetry.emit({ eventName: "worker.processing.latency", body: processingLatencyMs });
      this.telemetry.emit({
        eventName: "worker.item.completed",
        body: { itemId: queued.item.id, succeeded: !queued.item.shouldFail },
      });
      processed.push(queued.item.id);
    }
    return processed;
  }
}
