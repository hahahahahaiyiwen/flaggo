export function serviceLogQuery(name) {
  const domainCorrelated = name === "flaggo-evidence-materializer";
  const successHasNoLog = name === "flaggo-decision-service";
  return `{service_name=${JSON.stringify(name)}}`
    + (domainCorrelated || successHasNoLog ? "" : ' | trace_id != ""');
}

export function hasWorkloadTelemetry(name, { logs, spans, correlatedTrace }) {
  if (name === "flaggo-decision-service") {
    return spans.some((span) => span.name === "flaggo.decision.evaluate");
  }
  if (name === "flaggo-evidence-materializer") {
    return spans.some((span) => span.name === "flaggo.materializer.page")
      && logs.some((log) => log.eventName === "flaggo.materializer.page.committed"
        && log.attributes["flaggo.operation.outcome"] === "success"
        && typeof log.attributes["flaggo.batch.id"] === "string"
        && /^[1-9]\d*$/u.test(log.attributes["flaggo.batch.id"]));
  }
  return logs.length > 0 && spans.length > 0 && correlatedTrace !== undefined;
}
