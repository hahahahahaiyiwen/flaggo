import {
  SpanStatusCode,
  type Gauge,
  type Histogram,
  type Span,
} from "@opentelemetry/api";
import {
  createFlaggoTelemetry,
  type DecisionClient,
  type DecisionSpec,
  type RuntimeDecision,
} from "@flaggo/sdk/runtime";
import type { ApplicationTelemetry } from "./telemetry.js";
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
  processingLatencyMs: number;
}

export type WorkerDecisions = {
  readonly "demo.workerBatchSize": DecisionSpec<{
    readonly workerId: string;
    readonly cohort: string;
    readonly queuePressure: number;
    readonly processingLatencyMs: number;
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
  private readonly flaggoTelemetry;
  private readonly queueDepthMetric: Gauge;
  private readonly queuePressureMetric: Gauge;
  private readonly processingLatencyMetric: Histogram;
  private readonly batchSizeMetric: Gauge;
  private tickNumber = 0;

  constructor(
    private readonly flaggo: DecisionClient<WorkerDecisions>,
    private readonly telemetry: ApplicationTelemetry,
    private readonly clock: WorkerClock = new DeterministicWorkerClock(),
    private readonly queueCapacity = 8,
    private readonly targetLatencyMs = 100,
    private readonly workerId = "adaptive-worker-1",
    private readonly claimedCohort = "worker-canary",
  ) {
    this.flaggoTelemetry = createFlaggoTelemetry({
      logger: telemetry.flaggoLogger,
    });
    this.queueDepthMetric = telemetry.meter.createGauge("worker.queue.depth", {
      description: "Current queued work items.",
      unit: "{item}",
    });
    this.queuePressureMetric = telemetry.meter.createGauge(
      "worker.queue.pressure",
      {
        description: "Normalized worker queue pressure.",
        unit: "1",
      },
    );
    this.processingLatencyMetric = telemetry.meter.createHistogram(
      "worker.processing.latency",
      {
        description: "End-to-end work item processing latency.",
        unit: "ms",
      },
    );
    this.batchSizeMetric = telemetry.meter.createGauge("worker.batch.size", {
      description: "Batch size selected for the current worker tick.",
      unit: "{item}",
    });
  }

  get queueDepth(): number {
    return this.queue.length;
  }

  async runTick(profile: WorkloadProfile): Promise<WorkerTickResult> {
    return this.telemetry.tracer.startActiveSpan(
      "worker.tick",
      {
        attributes: {
          "worker.id": this.workerId,
          "worker.cohort": this.claimedCohort,
          "worker.profile": profile,
        },
      },
      async (span) => this.runTickInSpan(profile, span),
    );
  }

  private async runTickInSpan(
    profile: WorkloadProfile,
    span: Span,
  ): Promise<WorkerTickResult> {
    try {
      this.enqueue(profile);
      const queueDepthBefore = this.queue.length;
      this.queueDepthMetric.record(queueDepthBefore, {
        "worker.id": this.workerId,
        "worker.profile": profile,
      });
      const queuePressure = this.calculateQueuePressure();
      this.queuePressureMetric.record(queuePressure, {
        "worker.id": this.workerId,
        "worker.profile": profile,
      });
      span.setAttributes({
        "worker.queue.depth.before": queueDepthBefore,
        "worker.queue.pressure": queuePressure,
      });
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
      const processed = this.applyBatch(decision.result);
      const processedItemIds = processed.itemIds;
      operations.push(`batch-applied:${decision.result}`);
      const appliedAt = new Date().toISOString();
      this.telemetry.logger.emit({
        eventName: "worker.batch.applied",
        attributes: {
          "worker.batch.size": decision.result,
          "worker.batch.processed_count": processedItemIds.length,
          "worker.id": this.workerId,
          "worker.profile": profile,
        },
      });
      this.batchSizeMetric.record(decision.result, {
        "worker.id": this.workerId,
        "worker.profile": profile,
      });
      this.flaggoTelemetry.recordOutcome({
        binding: "demo.workerBatchSize.processingLatencyMs",
        value: processed.processingLatencyMs,
        contractName: "demo.workerBatchSize",
        contractDigest: decision.contractDigest,
        correlation: {
          workerId: this.workerId,
          cohort: this.claimedCohort,
          queuePressure,
        },
      });

      this.queueDepthMetric.record(this.queue.length, {
        "worker.id": this.workerId,
        "worker.profile": profile,
      });
      span.setAttributes({
        "worker.queue.depth.after": this.queue.length,
        "worker.batch.size": decision.result,
        "worker.batch.processed_count": processedItemIds.length,
        "flaggo.contract.digest": decision.contractDigest,
      });
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
        processingLatencyMs: processed.processingLatencyMs,
      };
    } catch (error) {
      span.recordException(error instanceof Error ? error : String(error));
      span.setStatus({
        code: SpanStatusCode.ERROR,
        message: error instanceof Error ? error.message : String(error),
      });
      throw error;
    } finally {
      span.end();
    }
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
      this.telemetry.logger.emit({
        eventName: "worker.item.enqueued",
        attributes: {
          "worker.item.id": item.id,
          "worker.item.processing_ms": item.processingMs,
          "worker.item.should_fail": item.shouldFail,
        },
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

  private applyBatch(batchSize: number): {
    itemIds: string[];
    processingLatencyMs: number;
  } {
    const itemIds: string[] = [];
    const latencies: number[] = [];
    for (let index = 0; index < batchSize; index += 1) {
      const queued = this.queue.shift();
      if (queued === undefined) break;
      this.clock.advance(queued.item.processingMs);
      const processingLatencyMs =
        this.clock.now() - queued.enqueuedAtMs;
      latencies.push(processingLatencyMs);
      this.processingLatencyMetric.record(processingLatencyMs, {
        "worker.id": this.workerId,
        "worker.item.succeeded": !queued.item.shouldFail,
      });
      this.telemetry.logger.emit({
        eventName: "worker.item.completed",
        attributes: {
          "worker.item.id": queued.item.id,
          "worker.item.succeeded": !queued.item.shouldFail,
          "worker.processing.latency_ms": processingLatencyMs,
        },
      });
      itemIds.push(queued.item.id);
    }
    return {
      itemIds,
      processingLatencyMs: latencies.length === 0
        ? 0
        : latencies.reduce((sum, value) => sum + value, 0) / latencies.length,
    };
  }
}
