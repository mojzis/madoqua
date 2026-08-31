//! The JSON wire formats: one record per run in the timing log, and the shape
//! `madoqua stats --json` emits.
//!
//! Every field name here is public contract. The log is append-only and gets
//! read back by a later version of this binary, so renaming a field breaks
//! history that has already been written; `docs/src/commands/` has to say the
//! same thing this file does.

use serde::{Deserialize, Serialize};

/// One line of `hook-timings.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// When the run finished, RFC 3339 in UTC.
    pub ts: String,
    /// The repository's directory name — written only when the log lives
    /// outside the repository, where runs from several repos mix together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Short sha of `HEAD`, the parent of the commit being made. Empty in a
    /// repository with no commits yet.
    pub head: String,
    /// How many staged Python files the run saw.
    pub files: usize,
    /// Wall-clock time for the whole hook.
    pub total_ms: u64,
    /// Whether madoqua had to put `.venv/bin` on `PATH` itself.
    pub venv_auto_activated: bool,
    /// Every step that ran, fixes first, then checks in configuration order.
    pub steps: Vec<StepRecord>,
}

/// One step within a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    /// The step's configured name.
    pub name: String,
    /// `"fix"` or `"check"`.
    pub phase: String,
    /// How long the tool itself took.
    pub ms: u64,
    /// The tool's exit code, or `-1` when madoqua killed it.
    pub exit: i32,
    /// Present only when the step ran out of time.
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out: bool,
}

#[expect(clippy::trivially_copy_pass_by_ref, reason = "serde's skip_serializing_if signature")]
fn is_false(value: &bool) -> bool {
    !*value
}

/// What `madoqua stats --json` emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsReport {
    /// The window the report covers, in days.
    pub days: u32,
    /// The repository filter that was applied, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// How many runs fell inside the window.
    pub runs: usize,
    /// Percentiles over whole-run durations.
    pub total: Summary,
    /// Percentiles per step, slowest by p95 first.
    pub steps: Vec<NamedSummary>,
}

/// Percentiles over one population of durations, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// How many samples the percentiles were computed from.
    pub n: usize,
    /// Median.
    pub p50: u64,
    /// 95th percentile — the number worth tuning against.
    pub p95: u64,
    /// The worst sample in the window.
    pub max: u64,
}

/// A [`Summary`] with the step name it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedSummary {
    /// The step's configured name.
    pub name: String,
    /// Its percentiles.
    #[serde(flatten)]
    pub summary: Summary,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> RunRecord {
        RunRecord {
            ts: "2026-08-31T12:03:22Z".to_owned(),
            repo: None,
            head: "a1b2c3d".to_owned(),
            files: 3,
            total_ms: 1834,
            venv_auto_activated: false,
            steps: vec![
                StepRecord {
                    name: "ruff fix".to_owned(),
                    phase: "fix".to_owned(),
                    ms: 41,
                    exit: 0,
                    timed_out: false,
                },
                StepRecord {
                    name: "ty".to_owned(),
                    phase: "check".to_owned(),
                    ms: 1620,
                    exit: 0,
                    timed_out: false,
                },
            ],
        }
    }

    #[test]
    fn a_record_round_trips_through_json() {
        let json = serde_json::to_string(&record()).unwrap();
        assert_eq!(
            serde_json::from_str::<RunRecord>(&json).unwrap(),
            record(),
            "the log has to survive being read back by a later run"
        );
    }

    #[test]
    fn the_quiet_fields_stay_out_of_the_common_case() {
        let json = serde_json::to_string(&record()).unwrap();
        assert!(!json.contains("timed_out"), "a step that finished says nothing about timing out");
        assert!(
            !json.contains("\"repo\""),
            "an in-repo log does not repeat the repo on every line"
        );
    }

    #[test]
    fn a_timed_out_step_is_marked_and_carries_minus_one() {
        let mut record = record();
        record.steps[1].timed_out = true;
        record.steps[1].exit = -1;

        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("\"timed_out\":true"), "got: {json}");
        assert!(json.contains("\"exit\":-1"), "got: {json}");
    }

    #[test]
    fn a_named_summary_is_flat_on_the_wire() {
        let json = serde_json::to_string(&NamedSummary {
            name: "ty".to_owned(),
            summary: Summary { n: 12, p50: 900, p95: 1600, max: 2000 },
        })
        .unwrap();
        assert_eq!(
            json, r#"{"name":"ty","n":12,"p50":900,"p95":1600,"max":2000}"#,
            "one row per step, not a nested object per step"
        );
    }
}
