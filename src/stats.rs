//! `madoqua stats`: what the timing log has to say.
//!
//! The point of the log is answering "which check is costing me the most
//! commits' worth of waiting", so the table sorts by p95 rather than by mean —
//! the mean of a check that is usually instant and occasionally 20 seconds
//! describes neither case.
//!
//! Everything here is pure: [`summarise`] takes the records and the current
//! time, so the window arithmetic is testable without waiting a month.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::clock;
use crate::report::{NamedSummary, RunRecord, StatsReport, Summary};

/// Seconds in a day.
const DAY: i64 = 86_400;

/// Build the report from the log, keeping runs inside the window.
///
/// Records whose timestamp cannot be read are dropped: they cannot be placed
/// in the window, and guessing would quietly skew the numbers.
pub fn summarise(records: &[RunRecord], days: u32, repo: Option<&str>, now: i64) -> StatsReport {
    let cutoff = now - i64::from(days) * DAY;

    let kept: Vec<&RunRecord> = records
        .iter()
        .filter(|record| repo.is_none_or(|wanted| record.repo.as_deref() == Some(wanted)))
        .filter(|record| clock::parse_rfc3339(&record.ts).is_some_and(|ts| ts >= cutoff))
        .collect();

    let mut per_step: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for record in &kept {
        for step in &record.steps {
            per_step.entry(step.name.as_str()).or_default().push(step.ms);
        }
    }

    let mut steps: Vec<NamedSummary> = per_step
        .into_iter()
        .map(|(name, samples)| NamedSummary { name: name.to_owned(), summary: summary(samples) })
        .collect();
    // p95 descending, then by name so equal rows do not shuffle between runs.
    steps.sort_by(|a, b| b.summary.p95.cmp(&a.summary.p95).then_with(|| a.name.cmp(&b.name)));

    StatsReport {
        days,
        repo: repo.map(ToOwned::to_owned),
        runs: kept.len(),
        total: summary(kept.iter().map(|record| record.total_ms).collect()),
        steps,
    }
}

/// Nearest-rank percentiles, which always return a value that actually
/// happened rather than an interpolation between two that did.
fn summary(mut samples: Vec<u64>) -> Summary {
    samples.sort_unstable();
    Summary {
        n: samples.len(),
        p50: percentile(&samples, 0.50),
        p95: percentile(&samples, 0.95),
        max: samples.last().copied().unwrap_or(0),
    }
}

fn percentile(sorted: &[u64], fraction: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the product of a sample count and a fraction in (0, 1] fits a usize"
    )]
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Render the report as a plain aligned table.
pub fn render(report: &StatsReport) -> String {
    if report.runs == 0 {
        return match &report.repo {
            Some(repo) => format!("no runs for {repo} in the last {} days\n", report.days),
            None => format!("no runs in the last {} days\n", report.days),
        };
    }

    let mut rows: Vec<[String; 5]> = vec![[
        "step".to_owned(),
        "n".to_owned(),
        "p50".to_owned(),
        "p95".to_owned(),
        "max".to_owned(),
    ]];
    rows.extend(report.steps.iter().map(|step| row(&step.name, step.summary)));
    rows.push(row("total run", report.total));

    let widths: Vec<usize> =
        (0..5).map(|column| rows.iter().map(|r| r[column].len()).max().unwrap_or(0)).collect();

    let last = rows.len() - 1;
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        // A blank line sets the whole-run summary apart from the steps it is
        // not the sum of — checks run in parallel.
        if index == last {
            out.push('\n');
        }
        let _ = write!(out, "{:<width$}", row[0], width = widths[0]);
        for (cell, width) in row.iter().zip(&widths).skip(1) {
            let _ = write!(out, "  {cell:>width$}");
        }
        out.push('\n');
    }
    out
}

fn row(name: &str, summary: Summary) -> [String; 5] {
    [
        name.to_owned(),
        summary.n.to_string(),
        summary.p50.to_string(),
        summary.p95.to_string(),
        summary.max.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::StepRecord;

    const NOW: i64 = 1_788_177_802; // 2026-08-31T12:03:22Z

    fn step(name: &str, ms: u64) -> StepRecord {
        StepRecord {
            name: name.to_owned(),
            phase: "check".to_owned(),
            ms,
            exit: 0,
            timed_out: false,
        }
    }

    fn run(days_ago: i64, total_ms: u64, steps: Vec<StepRecord>) -> RunRecord {
        RunRecord {
            ts: clock::format_rfc3339_utc(NOW - days_ago * DAY),
            repo: None,
            head: "abc1234".to_owned(),
            files: 1,
            total_ms,
            venv_auto_activated: false,
            steps,
        }
    }

    #[test]
    fn percentiles_are_nearest_rank_over_the_sorted_samples() {
        let samples: Vec<u64> = (1..=100).collect();
        let summary = summary(samples);
        assert_eq!(summary.p50, 50, "the 50th of 100 samples is the 50th value");
        assert_eq!(summary.p95, 95);
        assert_eq!(summary.max, 100);
        assert_eq!(summary.n, 100);
    }

    #[test]
    fn a_single_sample_is_its_own_every_percentile() {
        assert_eq!(summary(vec![7]), Summary { n: 1, p50: 7, p95: 7, max: 7 });
    }

    #[test]
    fn runs_outside_the_window_are_excluded() {
        let records = [run(1, 100, vec![]), run(29, 200, vec![]), run(45, 300, vec![])];
        let report = summarise(&records, 30, None, NOW);

        assert_eq!(report.runs, 2, "the 45-day-old run is outside a 30-day window");
        assert_eq!(report.total.max, 200, "and its duration is not in the numbers either");
    }

    #[test]
    fn steps_are_sorted_by_p95_descending() {
        let records = [
            run(0, 100, vec![step("fast", 10), step("slow", 900)]),
            run(0, 100, vec![step("fast", 12), step("slow", 800)]),
        ];
        let report = summarise(&records, 30, None, NOW);

        assert_eq!(
            report.steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["slow", "fast"],
            "the expensive step goes first, because that is the one worth fixing"
        );
        assert_eq!(report.steps[0].summary, Summary { n: 2, p50: 800, p95: 900, max: 900 });
    }

    #[test]
    fn the_repo_filter_keeps_only_that_repos_runs() {
        let mut mine = run(0, 100, vec![step("ty", 10)]);
        mine.repo = Some("mine".to_owned());
        let mut theirs = run(0, 900, vec![step("ty", 900)]);
        theirs.repo = Some("theirs".to_owned());

        let report = summarise(&[mine, theirs], 30, Some("mine"), NOW);
        assert_eq!(report.runs, 1);
        assert_eq!(report.total.max, 100, "the other repo's slow run is not mine to worry about");
        assert_eq!(report.repo.as_deref(), Some("mine"), "the report says what it was filtered to");
    }

    #[test]
    fn a_record_with_an_unreadable_timestamp_is_dropped() {
        let mut broken = run(0, 100, vec![]);
        broken.ts = "whenever".to_owned();
        assert_eq!(
            summarise(&[broken], 30, None, NOW).runs,
            0,
            "a run that cannot be placed in the window is not counted in it"
        );
    }

    #[test]
    fn an_empty_window_renders_a_sentence_rather_than_an_empty_table() {
        let report = summarise(&[], 30, None, NOW);
        assert_eq!(render(&report), "no runs in the last 30 days\n");
    }

    #[test]
    fn the_table_is_aligned_and_ends_with_the_whole_run_summary() {
        let records = [
            run(0, 1000, vec![step("ty check", 900), step("ruff check", 40)]),
            run(0, 1100, vec![step("ty check", 950), step("ruff check", 45)]),
        ];
        let rendered = render(&summarise(&records, 30, None, NOW));
        let lines: Vec<&str> = rendered.lines().collect();

        assert_eq!(lines[0], "step        n   p50   p95   max", "got: {rendered}");
        assert_eq!(lines[1], "ty check    2   900   950   950");
        assert_eq!(lines[2], "ruff check  2    40    45    45");
        assert_eq!(
            lines[3], "",
            "the run total is not the sum of parallel steps, so it is set apart"
        );
        assert_eq!(lines[4], "total run   2  1000  1100  1100");
    }
}
