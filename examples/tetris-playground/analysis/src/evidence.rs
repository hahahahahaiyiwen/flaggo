use std::collections::{BTreeMap, BTreeSet};

use flaggo_analysis_domain::AnalysisError;
use serde_json::{Value, json};

const MINIMUM_EPISODES: usize = 9;
const MINIMUM_EXPOSURES_PER_ARM: usize = 3;
const MINIMUM_PREDICTED_GAIN: f64 = 0.025;
const BALANCE: &str = "tetris.experience.challenge_balance";
const LINES: &str = "tetris.experience.lines_cleared";
const GAME_OVERS: &str = "tetris.experience.game_over_count";
const ACTIVE_PLAY: &str = "tetris.experience.active_play_ms";

#[derive(Debug)]
pub(crate) struct EvidenceAnalysis {
    pub(crate) findings: String,
    pub(crate) proposal: Result<Value, String>,
}

#[derive(Clone, Debug, PartialEq)]
struct WindowContext {
    pressure: f64,
    hard_drop_ratio: f64,
    pieces_locked: f64,
    interval: u64,
    start_ms: f64,
    end_ms: f64,
    complete: bool,
}

struct Window {
    context: WindowContext,
    values: BTreeMap<String, f64>,
}

#[derive(Default)]
struct Sample {
    pressure: f64,
    hard_drop_ratio: f64,
    interval: u64,
    active_ms: f64,
    balanced_ms: f64,
    lines: f64,
    game_overs: f64,
}

#[derive(Clone, Copy, Default)]
struct Statistics {
    count: usize,
    active_ms: f64,
    balanced_ms: f64,
    lines: f64,
    game_overs: f64,
}

impl Statistics {
    fn collect<'a>(samples: impl Iterator<Item = &'a Sample>) -> Self {
        samples.fold(Self::default(), |mut total, sample| {
            total.count += 1;
            total.active_ms += sample.active_ms;
            total.balanced_ms += sample.balanced_ms;
            total.lines += sample.lines;
            total.game_overs += sample.game_overs;
            total
        })
    }

    fn balance(self) -> f64 {
        self.balanced_ms / self.active_ms
    }

    fn lines_per_minute(self) -> f64 {
        self.lines * 60_000.0 / self.active_ms
    }

    fn game_overs_per_minute(self) -> f64 {
        self.game_overs * 60_000.0 / self.active_ms
    }
}

struct Policy {
    pressure: f64,
    hard_drop_ratio: f64,
    low_interval: u64,
    other_interval: u64,
    predicted_balance: f64,
    predicted_lines_per_minute: f64,
    predicted_game_overs_per_minute: f64,
}

impl Policy {
    fn rules(&self) -> Value {
        let mut rules = vec![json!({
            "name": "retain-current-without-observations",
            "when": {
                "expression": "attributes.active_play_ms_5s == 0.0 || attributes.pieces_locked_5s == 0"
            },
            "return": {"expression": "attributes.current_drop_interval_ms"}
        })];
        if self.low_interval != self.other_interval {
            let mut expression = format!("attributes.board_pressure_mean_5s < {}", self.pressure);
            if self.hard_drop_ratio > 0.0 {
                expression.push_str(&format!(
                    " && attributes.hard_drop_ratio_5s >= {}",
                    self.hard_drop_ratio
                ));
            }
            rules.push(json!({
                "name": "learned-low-pressure",
                "when": {"expression": expression},
                "return": {"value": self.low_interval}
            }));
        }
        rules.push(json!({
            "name": "learned-other-context",
            "when": {"expression": "true"},
            "return": {"value": self.other_interval}
        }));
        Value::Array(rules)
    }
}

pub(crate) fn analyze_evidence(result: &Value) -> Result<EvidenceAnalysis, AnalysisError> {
    let rows = result
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| evidence_error("query result has no rows"))?;
    let mut decisions = BTreeMap::<(String, String), u64>::new();
    let mut windows = BTreeMap::<(String, String, String), Window>::new();
    for row in rows {
        let name = row
            .get("signal_name")
            .and_then(Value::as_str)
            .ok_or_else(|| evidence_error("query row has no signal_name"))?;
        let text = row
            .get("payload_json")
            .and_then(Value::as_str)
            .ok_or_else(|| evidence_error("query row has no payload_json"))?;
        let payload: Value =
            serde_json::from_str(text).map_err(|error| evidence_error(&error.to_string()))?;
        if name == "flaggo.decision.received" {
            let attributes = payload.pointer("/signal/payload/attributes");
            let Some(session) = string_attribute(attributes, "flaggo.correlation.session_id")
            else {
                continue;
            };
            let Some(exposure) = string_attribute(attributes, "flaggo.correlation.exposure_id")
            else {
                continue;
            };
            let interval = string_attribute(attributes, "flaggo.result.json")
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| valid_interval(*value))
                .ok_or_else(|| evidence_error("decision has no contract-valid scalar interval"))?;
            let key = (session.to_owned(), exposure.to_owned());
            if let Some(previous) = decisions.insert(key, interval)
                && previous != interval
            {
                return Err(evidence_error(
                    "conflicting decision receipts for one exposure",
                ));
            }
            continue;
        }
        if ![BALANCE, LINES, GAME_OVERS, ACTIVE_PLAY].contains(&name) {
            continue;
        }
        let point = payload
            .pointer("/signal/payload/dataPoint")
            .ok_or_else(|| evidence_error("experience metric has no dataPoint"))?;
        let attributes = point.get("attributes");
        if string_attribute(attributes, "tetris.experience.dataset") != Some("training") {
            continue;
        }
        let session = required_string(attributes, "tetris.session.id")?;
        let exposure = required_string(attributes, "tetris.exposure.id")?;
        let id = required_string(attributes, "tetris.experience.window.id")?;
        let context = window_context(attributes)?;
        let value = number_point_value(point)
            .ok_or_else(|| evidence_error("experience gauge has no finite value"))?;
        if value < 0.0
            || (name == BALANCE && value > 1.0)
            || (name == ACTIVE_PLAY && value > 5_000.0)
            || ([LINES, GAME_OVERS].contains(&name) && value.fract() != 0.0)
        {
            return Err(evidence_error(
                "experience gauge is outside its value semantics",
            ));
        }
        let key = (session.to_owned(), exposure.to_owned(), id.to_owned());
        let window = windows.entry(key).or_insert_with(|| Window {
            context: context.clone(),
            values: BTreeMap::new(),
        });
        if window.context != context {
            return Err(evidence_error("re-exported window has conflicting context"));
        }
        if let Some(previous) = window.values.insert(name.to_owned(), value)
            && previous != value
        {
            return Err(evidence_error(
                "re-exported completed window has conflicting values",
            ));
        }
    }
    let mut completed = BTreeSet::new();
    let mut samples = BTreeMap::<(String, String), Sample>::new();
    let mut incomplete_windows = 0;
    for ((session, exposure, _), window) in windows {
        let key = (session.clone(), exposure);
        let Some(interval) = decisions.get(&key) else {
            incomplete_windows += 1;
            continue;
        };
        if window.values.len() != 4 {
            incomplete_windows += 1;
            continue;
        }
        if *interval != window.context.interval {
            return Err(evidence_error(
                "applied interval does not match its SDK receipt",
            ));
        }
        let active = window.values[ACTIVE_PLAY];
        if (window.context.end_ms - window.context.start_ms - active).abs() > 0.000_001 {
            return Err(evidence_error("window bounds do not match active duration"));
        }
        if window.context.complete {
            completed.insert(session);
        }
        if window.context.pieces_locked == 0.0 {
            continue;
        }
        let sample = samples.entry(key).or_insert_with(|| Sample {
            pressure: window.context.pressure,
            hard_drop_ratio: window.context.hard_drop_ratio,
            interval: *interval,
            ..Sample::default()
        });
        if sample.pressure != window.context.pressure
            || sample.hard_drop_ratio != window.context.hard_drop_ratio
        {
            return Err(evidence_error(
                "one exposure has conflicting before-call context",
            ));
        }
        sample.active_ms += active;
        sample.balanced_ms += window.values[BALANCE] * active;
        sample.lines += window.values[LINES];
        sample.game_overs += window.values[GAME_OVERS];
    }
    let samples: Vec<_> = samples
        .into_values()
        .filter(|sample| sample.active_ms > 0.0)
        .collect();
    let statistics = Statistics::collect(samples.iter());
    let policy = if completed.len() < MINIMUM_EPISODES {
        Err(format!(
            "waiting for {MINIMUM_EPISODES} completed training episodes (observed {})",
            completed.len()
        ))
    } else {
        fit_policy(&samples)
    };
    let baseline = if statistics.active_ms > 0.0 {
        format!(
            "{:.1}% balance; {:.2} lines/min; {:.2} game overs/min",
            statistics.balance() * 100.0,
            statistics.lines_per_minute(),
            statistics.game_overs_per_minute()
        )
    } else {
        "no scored active play".to_owned()
    };
    let outcome = match &policy {
        Ok(policy) => format!(
            "Candidate: pressure < {:.2}, hard-drop ratio >= {:.2}: {}ms; otherwise {}ms.\n\
             Predicted {:.1}% balance, {:.2} lines/min, {:.2} game overs/min.\n\
             These observational estimates require a separate matched held-out comparison.",
            policy.pressure,
            policy.hard_drop_ratio,
            policy.low_interval,
            policy.other_interval,
            policy.predicted_balance * 100.0,
            policy.predicted_lines_per_minute,
            policy.predicted_game_overs_per_minute
        ),
        Err(reason) => format!("No Candidate: {reason}"),
    };
    Ok(EvidenceAnalysis {
        findings: format!(
            "# Adaptive Tetris analysis\n\n\
             Completed training episodes: {}.\n\
             Independent scored exposures: {}; incomplete/unmatched windows: {}.\n\
             Observed exploratory baseline: {}.\n\n{}\n",
            completed.len(),
            samples.len(),
            incomplete_windows,
            baseline,
            outcome
        ),
        proposal: policy.map(|policy| policy.rules()),
    })
}

fn fit_policy(samples: &[Sample]) -> Result<Policy, String> {
    let arms: BTreeSet<_> = samples.iter().map(|sample| sample.interval).collect();
    if arms.len() != 2 {
        return Err("need comparable observations from both exploratory intervals".to_owned());
    }
    let intervals: Vec<_> = arms.into_iter().collect();
    let baseline = Statistics::collect(samples.iter());
    let mut best: Option<Policy> = None;
    for step in 1..=10 {
        let pressure = f64::from(step) / 20.0;
        for hard_drop_ratio in [0.0, 0.5, 0.75] {
            let low = |sample: &&Sample| {
                sample.pressure < pressure && sample.hard_drop_ratio >= hard_drop_ratio
            };
            let cohorts = [
                samples.iter().filter(low).collect::<Vec<_>>(),
                samples
                    .iter()
                    .filter(|sample| !low(sample))
                    .collect::<Vec<_>>(),
            ];
            let mut chosen = Vec::new();
            let mut weights = Vec::new();
            for cohort in &cohorts {
                let first = Statistics::collect(
                    cohort
                        .iter()
                        .copied()
                        .filter(|sample| sample.interval == intervals[0]),
                );
                let second = Statistics::collect(
                    cohort
                        .iter()
                        .copied()
                        .filter(|sample| sample.interval == intervals[1]),
                );
                if first.count < MINIMUM_EXPOSURES_PER_ARM
                    || second.count < MINIMUM_EXPOSURES_PER_ARM
                {
                    break;
                }
                chosen.push(if first.balance() > second.balance() + 1e-9 {
                    (intervals[0], first)
                } else {
                    (intervals[1], second)
                });
                weights.push((first.active_ms + second.active_ms) / baseline.active_ms);
            }
            if chosen.len() != 2 {
                continue;
            }
            let balance = weights
                .iter()
                .zip(&chosen)
                .map(|(weight, (_, stats))| weight * stats.balance())
                .sum::<f64>();
            let lines = weights
                .iter()
                .zip(&chosen)
                .map(|(weight, (_, stats))| weight * stats.lines_per_minute())
                .sum::<f64>();
            let game_overs = weights
                .iter()
                .zip(&chosen)
                .map(|(weight, (_, stats))| weight * stats.game_overs_per_minute())
                .sum::<f64>();
            if balance < baseline.balance() + MINIMUM_PREDICTED_GAIN
                || lines < 1.0
                || game_overs > 2.0
                || best
                    .as_ref()
                    .is_some_and(|policy| balance <= policy.predicted_balance + 1e-9)
            {
                continue;
            }
            best = Some(Policy {
                pressure,
                hard_drop_ratio,
                low_interval: chosen[0].0,
                other_interval: chosen[1].0,
                predicted_balance: balance,
                predicted_lines_per_minute: lines,
                predicted_game_overs_per_minute: game_overs,
            });
        }
    }
    best.ok_or_else(|| {
        "insufficient comparable context coverage, ambiguous improvement, or unsafe throughput/survival estimates".to_owned()
    })
}

fn window_context(attributes: Option<&Value>) -> Result<WindowContext, AnalysisError> {
    let pressure = required_number(attributes, "tetris.input.board_pressure_mean_5s")?;
    let hard_drop_ratio = required_number(attributes, "tetris.input.hard_drop_ratio_5s")?;
    let pieces_locked = required_number(attributes, "tetris.input.pieces_locked_5s")?;
    let interval = required_number(attributes, "tetris.drop_interval.ms")?;
    let start_ms = required_number(attributes, "tetris.experience.window.start_ms")?;
    let end_ms = required_number(attributes, "tetris.experience.window.end_ms")?;
    let complete = attribute_value(attributes, "tetris.experience.episode_complete")
        .and_then(Value::as_bool)
        .ok_or_else(|| evidence_error("window has no episode-completion flag"))?;
    if !(0.0..=1.0).contains(&pressure)
        || !(0.0..=1.0).contains(&hard_drop_ratio)
        || pieces_locked < 0.0
        || pieces_locked.fract() != 0.0
        || interval.fract() != 0.0
        || !(200.0..=1500.0).contains(&interval)
        || !valid_interval(interval as u64)
        || end_ms < start_ms
        || end_ms - start_ms > 5_000.0
    {
        return Err(evidence_error(
            "window context is outside its declared semantics",
        ));
    }
    Ok(WindowContext {
        pressure,
        hard_drop_ratio,
        pieces_locked,
        interval: interval as u64,
        start_ms,
        end_ms,
        complete,
    })
}

fn valid_interval(value: u64) -> bool {
    (200..=1500).contains(&value) && value.is_multiple_of(50)
}

fn evidence_error(message: &str) -> AnalysisError {
    AnalysisError::Evidence(message.to_owned())
}

fn attribute_value<'a>(attributes: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    attributes?
        .as_array()?
        .iter()
        .find(|attribute| attribute.get("key").and_then(Value::as_str) == Some(name))?
        .pointer("/value/value")
}

fn string_attribute<'a>(attributes: Option<&'a Value>, name: &str) -> Option<&'a str> {
    attribute_value(attributes, name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn required_string<'a>(
    attributes: Option<&'a Value>,
    name: &str,
) -> Result<&'a str, AnalysisError> {
    string_attribute(attributes, name)
        .ok_or_else(|| evidence_error(&format!("window has no '{name}'")))
}

fn required_number(attributes: Option<&Value>, name: &str) -> Result<f64, AnalysisError> {
    let value = attribute_value(attributes, name)
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|value: &f64| value.is_finite());
    value.ok_or_else(|| evidence_error(&format!("window has no finite '{name}'")))
}

fn number_point_value(point: &Value) -> Option<f64> {
    let value = point.get("value")?;
    value
        .get("value")
        .and_then(Value::as_f64)
        .or_else(|| value.get("int64").and_then(Value::as_str)?.parse().ok())
        .filter(|value: &f64| value.is_finite())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{ACTIVE_PLAY, BALANCE, GAME_OVERS, LINES, analyze_evidence};

    #[test]
    fn learns_a_contextual_policy_from_forward_outcomes() {
        let analysis = analyze_evidence(&fixture(false)).expect("analysis");
        let rules = analysis.proposal.expect("Candidate");
        assert_eq!(
            rules[0]["return"]["expression"],
            "attributes.current_drop_interval_ms"
        );
        assert_eq!(
            rules[0]["when"]["expression"],
            "attributes.active_play_ms_5s == 0.0 || attributes.pieces_locked_5s == 0"
        );
        assert_eq!(
            rules[1]["when"]["expression"],
            "attributes.board_pressure_mean_5s < 0.15"
        );
        assert_eq!(rules[1]["return"]["value"], 200);
        assert_eq!(rules[2]["return"]["value"], 1000);
        assert!(
            analysis
                .findings
                .contains("Independent scored exposures: 12")
        );
    }

    #[test]
    fn does_not_encode_the_expected_faster_interval_as_the_winner() {
        let rules = analyze_evidence(&fixture(true))
            .expect("analysis")
            .proposal
            .expect("Candidate");
        assert_eq!(rules[1]["return"]["value"], 1000);
        assert_eq!(rules[2]["return"]["value"], 200);
    }

    #[test]
    fn repeated_gauge_exports_do_not_create_samples() {
        let original = fixture(false);
        let mut repeated = original.clone();
        repeated["rows"]
            .as_array_mut()
            .expect("rows")
            .extend(original["rows"].as_array().expect("rows").iter().cloned());
        let first = analyze_evidence(&original).expect("analysis");
        let second = analyze_evidence(&repeated).expect("analysis");
        assert_eq!(first.proposal, second.proposal);
        assert_eq!(first.findings, second.findings);
    }

    #[test]
    fn requires_completed_episode_coverage_not_just_a_new_digest() {
        let mut evidence = fixture(false);
        evidence["rows"]
            .as_array_mut()
            .expect("rows")
            .retain(|row| {
                if row["signal_name"] == "flaggo.decision.received" {
                    return true;
                }
                let payload: Value =
                    serde_json::from_str(row["payload_json"].as_str().expect("payload"))
                        .expect("JSON");
                !payload["signal"]["payload"]["dataPoint"]["attributes"]
                    .as_array()
                    .expect("attributes")
                    .iter()
                    .any(|attribute| {
                        attribute["key"] == "tetris.experience.episode_complete"
                            && attribute["value"]["value"] == true
                    })
            });
        assert!(
            analyze_evidence(&evidence)
                .expect("analysis")
                .proposal
                .expect_err("No Candidate")
                .contains("9 completed")
        );
    }

    #[test]
    fn cannot_credit_metrics_without_correlated_sdk_receipts() {
        let mut evidence = fixture(false);
        evidence["rows"]
            .as_array_mut()
            .expect("rows")
            .retain(|row| row["signal_name"] != "flaggo.decision.received");
        assert!(
            analyze_evidence(&evidence)
                .expect("analysis")
                .proposal
                .is_err()
        );
    }

    #[test]
    fn rejects_wrong_applied_result_and_conflicting_reexports() {
        let mut evidence = fixture(false);
        rewrite_payload(&mut evidence["rows"][0], |payload| {
            payload["signal"]["payload"]["attributes"][2]["value"]["value"] = json!("1000");
        });
        assert!(
            analyze_evidence(&evidence)
                .expect_err("bad result")
                .to_string()
                .contains("SDK receipt")
        );

        let mut evidence = fixture(false);
        let mut duplicate = evidence["rows"][1].clone();
        rewrite_payload(&mut duplicate, |payload| {
            payload["signal"]["payload"]["dataPoint"]["value"]["value"] = json!(0.1);
        });
        evidence["rows"]
            .as_array_mut()
            .expect("rows")
            .push(duplicate);
        assert!(
            analyze_evidence(&evidence)
                .expect_err("conflict")
                .to_string()
                .contains("conflicting values")
        );
    }

    #[test]
    fn blocks_zero_progress_and_ambiguous_outcomes() {
        for name in [LINES, BALANCE] {
            let mut evidence = fixture(false);
            for row in evidence["rows"].as_array_mut().expect("rows") {
                if row["signal_name"] == name {
                    rewrite_payload(row, |payload| {
                        payload["signal"]["payload"]["dataPoint"]["value"] = if name == LINES {
                            json!({"int64": "0"})
                        } else {
                            json!({"doubleBits": "0", "value": 0.5})
                        };
                    });
                }
            }
            assert!(
                analyze_evidence(&evidence)
                    .expect("analysis")
                    .proposal
                    .is_err()
            );
        }
    }

    #[test]
    fn zero_duration_completion_facts_do_not_add_quality_weight() {
        let original = analyze_evidence(&fixture(false)).expect("analysis");
        let mut evidence = fixture(false);
        for row in evidence["rows"].as_array_mut().expect("rows") {
            if row["signal_name"] == BALANCE {
                rewrite_payload(row, |payload| {
                    let point = &mut payload["signal"]["payload"]["dataPoint"];
                    let complete = point["attributes"]
                        .as_array()
                        .expect("attributes")
                        .iter()
                        .any(|a| {
                            a["key"] == "tetris.experience.episode_complete"
                                && a["value"]["value"] == true
                        });
                    if complete {
                        point["value"] = json!({
                            "doubleBits": format!("{:016x}", 1.0_f64.to_bits()),
                            "value": 1.0
                        });
                    }
                });
            }
        }
        let changed = analyze_evidence(&evidence).expect("analysis");
        assert_eq!(original.proposal, changed.proposal);
        assert_eq!(original.findings, changed.findings);
    }

    #[test]
    fn rejects_fabricated_completion_duration() {
        let mut evidence = fixture(false);
        for row in evidence["rows"].as_array_mut().expect("rows") {
            if row["signal_name"] == ACTIVE_PLAY {
                rewrite_payload(row, |payload| {
                    let point = &mut payload["signal"]["payload"]["dataPoint"];
                    if point["attributes"]
                        .as_array()
                        .expect("attributes")
                        .iter()
                        .any(|a| {
                            a["key"] == "tetris.experience.episode_complete"
                                && a["value"]["value"] == true
                        })
                    {
                        point["value"] = json!({"int64": "1"});
                    }
                });
            }
        }
        assert!(
            analyze_evidence(&evidence)
                .expect_err("bad duration")
                .to_string()
                .contains("active duration")
        );
    }

    fn fixture(reverse: bool) -> Value {
        let mut rows = Vec::new();
        for index in 0..12 {
            let low = index < 6;
            let fast = index % 6 < 3;
            let interval = if fast { 200 } else { 1000 };
            let session = format!("tetris-demo-{}", index % 9);
            let exposure = format!("decision-{index}");
            rows.push(row(
                "flaggo.decision.received",
                json!({
                    "signal": {"payload": {"attributes": [
                        attribute("flaggo.correlation.session_id", json!(session)),
                        attribute("flaggo.correlation.exposure_id", json!(exposure)),
                        attribute("flaggo.result.json", json!(interval.to_string()))
                    ]}}
                }),
            ));
            let preferred = (low == fast) != reverse;
            append_window(&mut rows, index, false, if preferred { 0.9 } else { 0.2 });
        }
        for session in 0..9 {
            let last = if session < 3 { session + 9 } else { session };
            append_window(&mut rows, last, true, 0.0);
        }
        json!({"rows": rows})
    }

    fn append_window(rows: &mut Vec<Value>, index: usize, complete: bool, balance: f64) {
        let start = (index / 9) * 5000 + if complete { 5000 } else { 0 };
        let active = if complete { 0 } else { 5000 };
        let attributes = json!([
            attribute(
                "tetris.session.id",
                json!(format!("tetris-demo-{}", index % 9))
            ),
            attribute("tetris.exposure.id", json!(format!("decision-{index}"))),
            attribute(
                "tetris.experience.window.id",
                json!(format!("window-{index}-{complete}"))
            ),
            attribute("tetris.experience.dataset", json!("training")),
            attribute("tetris.experience.episode_complete", json!(complete)),
            attribute("tetris.experience.window.start_ms", json!(start)),
            attribute("tetris.experience.window.end_ms", json!(start + active)),
            attribute(
                "tetris.drop_interval.ms",
                json!(if index % 6 < 3 { 200 } else { 1000 })
            ),
            attribute(
                "tetris.input.board_pressure_mean_5s",
                json!(if index < 6 { 0.1 } else { 0.3 })
            ),
            attribute("tetris.input.hard_drop_ratio_5s", json!(1.0)),
            attribute("tetris.input.pieces_locked_5s", json!(2))
        ]);
        for (name, value) in [
            (
                BALANCE,
                json!({"doubleBits": format!("{:016x}", balance.to_bits()), "value": balance}),
            ),
            (LINES, json!({"int64": if complete { "0" } else { "1" }})),
            (GAME_OVERS, json!({"int64": "0"})),
            (ACTIVE_PLAY, json!({"int64": active.to_string()})),
        ] {
            rows.push(row(
                name,
                json!({
                    "signal": {"payload": {"dataPoint": {"attributes": attributes, "value": value}}}
                }),
            ));
        }
    }

    fn attribute(key: &str, value: Value) -> Value {
        let encoded = match &value {
            Value::Bool(_) => json!({"type": "bool", "value": value}),
            Value::String(_) => json!({"type": "string", "value": value}),
            Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    json!({"type": "int64", "value": integer.to_string()})
                } else {
                    let double = number.as_f64().expect("finite number");
                    json!({"type": "double", "bits": format!("{:016x}", double.to_bits()), "value": double})
                }
            }
            _ => panic!("scalar fixture attribute"),
        };
        json!({"key": key, "keyStringTableIndex": 0, "value": encoded})
    }

    fn row(name: &str, payload: Value) -> Value {
        json!({"signal_name": name, "payload_json": serde_json::to_string(&payload).expect("JSON")})
    }

    fn rewrite_payload(row: &mut Value, update: impl FnOnce(&mut Value)) {
        let mut payload: Value =
            serde_json::from_str(row["payload_json"].as_str().expect("payload")).expect("JSON");
        update(&mut payload);
        row["payload_json"] = json!(serde_json::to_string(&payload).expect("JSON"));
    }
}
