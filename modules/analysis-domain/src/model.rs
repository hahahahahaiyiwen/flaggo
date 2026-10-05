use chrono::{DateTime, Months, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisAuthority {
    pub tenant: String,
    pub application: String,
    pub environment: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisEvidenceSource {
    pub kind: String,
    pub scope: String,
    pub name: String,
    #[serde(default)]
    pub metric_kind: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub span_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisContract {
    pub name: String,
    pub contract_digest: String,
    pub accepted_at: DateTime<Utc>,
    pub authority: AnalysisAuthority,
    pub evaluation_interval: String,
    pub evidence_sources: Vec<AnalysisEvidenceSource>,
    pub contract: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProfile {
    pub skill_name: String,
    pub skill_version: String,
}

impl AnalysisContract {
    pub fn eligible_after(&self, anchor: DateTime<Utc>) -> Result<DateTime<Utc>, ScheduleError> {
        ScheduleInterval::parse(&self.evaluation_interval)?.add_to(anchor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogUpdate {
    NotModified,
    Updated {
        revision: String,
        contracts: Vec<AnalysisContractIdentity>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCycle {
    pub workspace_id: String,
    pub cycle_id: String,
    pub contract_name: String,
    pub contract_digest: String,
    pub analysis_profile: AnalysisProfile,
    pub opened_at: DateTime<Utc>,
    pub resumed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptContext {
    pub workspace_id: String,
    pub cycle_id: String,
    pub attempt_id: String,
    pub contract: AnalysisContractIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisContractIdentity {
    pub name: String,
    pub digest: String,
}

impl AttemptContext {
    #[must_use]
    pub fn new(cycle: &AnalysisCycle, attempt_id: String) -> Self {
        Self {
            workspace_id: cycle.workspace_id.clone(),
            cycle_id: cycle.cycle_id.clone(),
            attempt_id,
            contract: AnalysisContractIdentity {
                name: cycle.contract_name.clone(),
                digest: cycle.contract_digest.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceCutoff {
    pub cutoff: DateTime<Utc>,
    pub watermark: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRecord {
    pub contract_name: String,
    pub contract_digest: String,
    pub executable_digest: String,
    pub created_at: DateTime<Utc>,
    pub created: bool,
    pub analysis_manifest_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateProposal {
    pub attempt_id: String,
    pub rules: Value,
    pub analysis_manifest_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CycleOutcome {
    Candidate { candidate: CandidateRecord },
    NoChange { reason: String },
    NoCandidate { reason: String },
    Failure { reason: String },
    Superseded { replacement_digest: Option<String> },
}

#[derive(Debug, Error)]
pub enum ScheduleError {
    #[error("learning evaluation interval is invalid: {0}")]
    InvalidInterval(String),
    #[error("learning evaluation interval exceeds the supported timestamp range")]
    Overflow,
}

#[derive(Default)]
struct ScheduleInterval {
    years: u32,
    months: u32,
    weeks: u64,
    days: u64,
    hours: u64,
    minutes: u64,
    seconds: u64,
    nanoseconds: u32,
}

impl ScheduleInterval {
    fn parse(value: &str) -> Result<Self, ScheduleError> {
        let body = value
            .strip_prefix('P')
            .ok_or_else(|| ScheduleError::InvalidInterval(value.to_owned()))?;
        if body.is_empty() {
            return Err(ScheduleError::InvalidInterval(value.to_owned()));
        }

        let (date, time) = match body.split_once('T') {
            Some((date, time)) if !time.is_empty() => (date, Some(time)),
            Some(_) => return Err(ScheduleError::InvalidInterval(value.to_owned())),
            None => (body, None),
        };
        let mut interval = Self::default();
        parse_date_components(date, value, &mut interval)?;
        if let Some(time) = time {
            parse_time_components(time, value, &mut interval)?;
        }
        if interval.is_zero() {
            return Err(ScheduleError::InvalidInterval(value.to_owned()));
        }
        Ok(interval)
    }

    fn is_zero(&self) -> bool {
        self.years == 0
            && self.months == 0
            && self.weeks == 0
            && self.days == 0
            && self.hours == 0
            && self.minutes == 0
            && self.seconds == 0
            && self.nanoseconds == 0
    }

    fn add_to(&self, anchor: DateTime<Utc>) -> Result<DateTime<Utc>, ScheduleError> {
        let calendar_months = self
            .years
            .checked_mul(12)
            .and_then(|years| years.checked_add(self.months))
            .ok_or(ScheduleError::Overflow)?;
        let mut result = anchor
            .checked_add_months(Months::new(calendar_months))
            .ok_or(ScheduleError::Overflow)?;

        let days = self
            .weeks
            .checked_mul(7)
            .and_then(|weeks| weeks.checked_add(self.days))
            .ok_or(ScheduleError::Overflow)?;
        result = add_time_delta(result, days, TimeDelta::try_days)?;
        result = add_time_delta(result, self.hours, TimeDelta::try_hours)?;
        result = add_time_delta(result, self.minutes, TimeDelta::try_minutes)?;
        result = add_time_delta(result, self.seconds, TimeDelta::try_seconds)?;
        result
            .checked_add_signed(TimeDelta::nanoseconds(i64::from(self.nanoseconds)))
            .ok_or(ScheduleError::Overflow)
    }
}

fn parse_date_components(
    value: &str,
    original: &str,
    interval: &mut ScheduleInterval,
) -> Result<(), ScheduleError> {
    if value.is_empty() {
        return Ok(());
    }
    let components = parse_components(value, &['Y', 'M', 'W', 'D'], original)?;
    for (designator, number) in components {
        match designator {
            'Y' => interval.years = parse_u32(number, original)?,
            'M' => interval.months = parse_u32(number, original)?,
            'W' => interval.weeks = parse_u64(number, original)?,
            'D' => interval.days = parse_u64(number, original)?,
            _ => return Err(ScheduleError::InvalidInterval(original.to_owned())),
        }
    }
    Ok(())
}

fn parse_time_components(
    value: &str,
    original: &str,
    interval: &mut ScheduleInterval,
) -> Result<(), ScheduleError> {
    let components = parse_components(value, &['H', 'M', 'S'], original)?;
    for (designator, number) in components {
        match designator {
            'H' => interval.hours = parse_u64(number, original)?,
            'M' => interval.minutes = parse_u64(number, original)?,
            'S' => {
                let (seconds, nanoseconds) = parse_seconds(number, original)?;
                interval.seconds = seconds;
                interval.nanoseconds = nanoseconds;
            }
            _ => return Err(ScheduleError::InvalidInterval(original.to_owned())),
        }
    }
    Ok(())
}

fn parse_components<'a>(
    value: &'a str,
    order: &[char],
    original: &str,
) -> Result<Vec<(char, &'a str)>, ScheduleError> {
    let mut result = Vec::new();
    let mut number_start = 0;
    let mut last_order = None;
    for (index, character) in value.char_indices() {
        if character.is_ascii_digit() || character == '.' {
            continue;
        }
        let number = &value[number_start..index];
        let position = order.iter().position(|expected| *expected == character);
        if number.is_empty()
            || position.is_none()
            || last_order.is_some_and(|last| position.is_some_and(|current| current <= last))
        {
            return Err(ScheduleError::InvalidInterval(original.to_owned()));
        }
        result.push((character, number));
        last_order = position;
        number_start = index + character.len_utf8();
    }
    if number_start != value.len() || result.is_empty() {
        return Err(ScheduleError::InvalidInterval(original.to_owned()));
    }
    Ok(result)
}

fn parse_u32(value: &str, original: &str) -> Result<u32, ScheduleError> {
    if value.contains('.') {
        return Err(ScheduleError::InvalidInterval(original.to_owned()));
    }
    value
        .parse()
        .map_err(|_| ScheduleError::InvalidInterval(original.to_owned()))
}

fn parse_u64(value: &str, original: &str) -> Result<u64, ScheduleError> {
    if value.contains('.') {
        return Err(ScheduleError::InvalidInterval(original.to_owned()));
    }
    value
        .parse()
        .map_err(|_| ScheduleError::InvalidInterval(original.to_owned()))
}

fn parse_seconds(value: &str, original: &str) -> Result<(u64, u32), ScheduleError> {
    let (whole, fraction) = match value.split_once('.') {
        Some((whole, fraction)) if !whole.is_empty() && !fraction.is_empty() => {
            (whole, Some(fraction))
        }
        Some(_) => return Err(ScheduleError::InvalidInterval(original.to_owned())),
        None => (value, None),
    };
    let seconds = parse_u64(whole, original)?;
    let Some(fraction) = fraction else {
        return Ok((seconds, 0));
    };
    if !fraction.chars().all(|character| character.is_ascii_digit()) {
        return Err(ScheduleError::InvalidInterval(original.to_owned()));
    }
    let significant = fraction.trim_end_matches('0');
    if significant.len() > 9 {
        return Err(ScheduleError::InvalidInterval(
            "fractional seconds exceed nanosecond precision".to_owned(),
        ));
    }
    let mut nanoseconds = significant.to_owned();
    nanoseconds.extend(std::iter::repeat_n('0', 9 - significant.len()));
    let parsed = nanoseconds
        .parse()
        .map_err(|_| ScheduleError::InvalidInterval(original.to_owned()))?;
    Ok((seconds, parsed))
}

fn add_time_delta(
    anchor: DateTime<Utc>,
    value: u64,
    constructor: fn(i64) -> Option<TimeDelta>,
) -> Result<DateTime<Utc>, ScheduleError> {
    let value = i64::try_from(value).map_err(|_| ScheduleError::Overflow)?;
    let delta = constructor(value).ok_or(ScheduleError::Overflow)?;
    anchor
        .checked_add_signed(delta)
        .ok_or(ScheduleError::Overflow)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::{AnalysisAuthority, AnalysisContract, ScheduleError};

    fn contract(interval: &str) -> AnalysisContract {
        AnalysisContract {
            name: "checkout.delay".to_owned(),
            contract_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            accepted_at: chrono::Utc
                .with_ymd_and_hms(2026, 1, 31, 12, 0, 0)
                .single()
                .unwrap(),
            authority: AnalysisAuthority {
                tenant: "local".to_owned(),
                application: "checkout".to_owned(),
                environment: "test".to_owned(),
            },
            evaluation_interval: interval.to_owned(),
            evidence_sources: Vec::new(),
            contract: json!({}),
        }
    }

    #[test]
    fn schedule_uses_contract_authored_calendar_duration() {
        let contract = contract("P1M");
        assert_eq!(
            contract
                .eligible_after(contract.accepted_at)
                .unwrap()
                .to_rfc3339(),
            "2026-02-28T12:00:00+00:00"
        );
    }

    #[test]
    fn schedule_rejects_invalid_interval() {
        assert!(matches!(
            contract("not-a-duration").eligible_after(contract("PT1M").accepted_at),
            Err(ScheduleError::InvalidInterval(_))
        ));
    }
}
