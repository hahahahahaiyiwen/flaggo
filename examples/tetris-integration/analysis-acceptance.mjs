export function createCandidateObserver({ allowDraftHandoff = false } = {}) {
  const pendingHandoffs = new Set();
  const handoffs = [];
  function attemptKey(entry) {
    const cycle = entry["flaggo.analysis.cycle.id"];
    const attempt = entry["flaggo.analysis.attempt.id"];
    return typeof cycle === "string" && cycle !== ""
      && typeof attempt === "string" && attempt !== ""
      ? `${cycle}/${attempt}` : undefined;
  }
  return {
    handoffs,
    predicate(entry) {
      const event = entry["event.name"];
      const key = attemptKey(entry);
      if (allowDraftHandoff && key !== undefined
        && event === "flaggo.analysis.action.completed"
        && entry["flaggo.analysis.action.kind"] === "session.run"
        && entry["flaggo.analysis.action.outcome"] === "handoff") {
        pendingHandoffs.add(key);
        return false;
      }
      if (event === "flaggo.analysis.cycle.failed" || event === "flaggo.service.failed") {
        if (allowDraftHandoff && key !== undefined
          && event === "flaggo.analysis.cycle.failed"
          && entry["flaggo.operation.outcome"] === "recoverable"
          && entry["flaggo.failure.category"] === "internal"
          && pendingHandoffs.delete(key)) {
          handoffs.push(entry);
          return false;
        }
        throw new Error(`Async Analysis failed: ${JSON.stringify(entry)}`);
      }
      if (event !== "flaggo.analysis.cycle.completed") return false;
      const outcome = entry["flaggo.operation.outcome"];
      if (outcome === "not_eligible" || outcome === "busy") return false;
      if (outcome === "candidate") return true;
      throw new Error(`Async Analysis did not produce a Candidate: ${JSON.stringify(entry)}`);
    },
  };
}
