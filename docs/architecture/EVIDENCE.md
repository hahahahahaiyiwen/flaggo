# Decision evidence

## Purpose

Evidence Store is the durable boundary for facts Flaggo observes or derives:
telemetry observations, evidence views, decision records, confirmed exposures,
and outcomes.

Evidence can inform Decision Service constraints and future Async Analysis
Pipeline candidates. It never creates or activates authority.

## Record kinds

| Kind | Meaning |
| --- | --- |
| Runtime inputs | Live typed values supplied with one decision request. |
| Observation | Event, metric, trace, log, span, or domain fact received through OTel Ingestion or another explicit adapter. |
| Evidence view | Target/window/filter projection over observations with freshness and quality. |
| Decision record | Reconstructable result appended by Decision Service before success. |
| Exposure record | Confirmation that the application applied or rendered a returned result. |
| Outcome | Observation attributed to a confirmed exposure. |

Runtime inputs travel with a request and are copied into its decision record.
They do not require hot-path aggregation from Evidence Store.

## Signal model

Decision definitions declare immutable keyed signals:

- manifest input declarations may select request or evidence sources through
  #44's typed resolution contract;
- events and derived metrics may inform evidence and objectives;
- objective signals must be numeric; and
- derived signal semantics include their aggregation expression and fixed
  window where applicable.

Every online input declares a request or evidence source through #44's
manifest contract. Request-sourced primitive values arrive with the call.
Evidence-sourced values are resolved from authorized materialized views before
bounded execution. Contract Service validates the declaration; Decision
Service consumes and records typed resolved values plus provenance.

## Evidence views

An evidence view identifies:

```text
signal + target + window + filters + freshness/quality
```

Views are reusable when those immutable semantics match. Raw observations and
signal definitions may also be reused across definition revisions. Decision
state is never reused implicitly because authority belongs to State Store and
an exact runtime identity.

## Decision, exposure, and outcome

```text
Decision Service returns value
  -> decision record already durable
  -> application applies or renders value
  -> explicit exposure confirmation
  -> exposure record
  -> later outcome references exposureId
```

A decision record answers what Flaggo returned. An exposure record answers
whether the application used it. An outcome answers what happened after that
confirmed use.

Confirmation cannot replace decision-time inputs. The server copies the
immutable attribution snapshot into the exposure record and returns the same
exposure identity on an exact retry.

Ordinary telemetry remains raw and unlinked. Attribution occurs only through
an explicit confirmed `exposureId`.

## OpenTelemetry relationship

The application owns its OpenTelemetry APIs, providers, exporter, and optional
Collector configuration. Flaggo owns OTel Ingestion and normalization into
Evidence Store.

| OTel signal | Evidence contribution |
| --- | --- |
| Metrics | Rates, averages, percentiles, counts, constraints, and objectives. |
| Traces | Request/workflow path and latency attribution. |
| Span events | Domain facts attached to operations. |
| Logs | Structured domain records and diagnostics. |
| Context | Target or correlation metadata. |

OpenTelemetry is a preferred transport and correlation model, not the only
internal evidence shape.

## Audit and explanation

Audit is the invariant that lifecycle and runtime outcomes are durable and
reconstructable. It is not a standalone service.

Explanation is a deterministic projection of stored facts such as:

- definition identity and targets;
- live inputs;
- state, activation, and approval lineage;
- selected candidate and applied constraints;
- fallback provenance;
- returned value and timestamp.

Explanation cannot invent missing provenance or reinterpret authority.

## Reuse and isolation

| Layer | Default behavior |
| --- | --- |
| Observations | Reusable when immutable signal semantics match. |
| Evidence views | Reusable when signal, target, window, and filters match. |
| Decision records | Bound to the exact identity and request. |
| Exposure records | Bound to one decision result and application use. |
| Outcomes | Bound to a confirmed exposure when attributed. |
| Decision state | Owned separately by State Store; never inferred from evidence. |

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Decision definition](DECISION_DEFINITION.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [OTel Ingestion](../design/otel-ingestion/README.md)
- [Evidence Store](../design/evidence-store/README.md)
- [Async Analysis Pipeline](../design/async-analysis/README.md)
