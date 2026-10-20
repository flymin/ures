use crate::{
    config,
    model::{CheckResult, Metric, Report, Status},
    util,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

fn metric(id: &str, value: Option<f64>, unit: &str) -> Metric {
    Metric {
        id: id.into(),
        value,
        unit: unit.into(),
    }
}
fn level(value: Option<f64>, check: &config::Check) -> Status {
    let Some(value) = value else {
        return Status::Unknown;
    };
    if check.critical.is_some_and(|t| value >= t) {
        Status::Critical
    } else if check.warn.is_some_and(|t| value >= t) {
        Status::Warn
    } else {
        Status::Ok
    }
}

pub fn builtins(config: &config::Config, report: &Report) -> Vec<CheckResult> {
    let s = &report.summary;
    let mut results = Vec::new();
    for (id, check) in config
        .checks
        .iter()
        .filter(|(_, c)| c.enabled && c.command.is_empty())
    {
        let (status, summary, metrics) = match id.as_str() {
            "cpu" => {
                let percent = s.cpu_limit.value.map(|v| s.cpu_cores / v * 100.0);
                let mut status = level(percent, check);
                if s.throttled_periods.unwrap_or(0) > 0
                    && matches!(status, Status::Ok | Status::Unknown)
                {
                    status = Status::Warn;
                }
                (
                    status,
                    format!(
                        "{:.2} cores; {}",
                        s.cpu_cores,
                        percent.map_or_else(
                            || "no user limit detected".into(),
                            |v| format!("{v:.1}% of {}", s.cpu_limit.source)
                        )
                    ),
                    vec![
                        metric("cores", Some(s.cpu_cores), "cores"),
                        metric("limit_percent", percent, "percent"),
                        metric(
                            "throttled_periods",
                            s.throttled_periods.map(|v| v as f64),
                            "count",
                        ),
                    ],
                )
            }
            "memory" => {
                let percent = s
                    .memory_limit
                    .value
                    .map(|v| s.memory_bytes as f64 / v * 100.0);
                (
                    level(percent, check),
                    format!(
                        "{}; {}",
                        util::size(s.memory_bytes),
                        percent.map_or_else(
                            || "no user limit detected".into(),
                            |v| format!("{v:.1}% of {}", s.memory_limit.source)
                        )
                    ),
                    vec![
                        metric("used", Some(s.memory_bytes as f64), "bytes"),
                        metric("limit_percent", percent, "percent"),
                    ],
                )
            }
            "oom" => {
                let status = match s.oom_new {
                    Some(n) if n > 0 => Status::Critical,
                    Some(_) => Status::Ok,
                    None if !report.oom.sources.is_empty() => Status::Ok,
                    None => Status::Unknown,
                };
                (
                    status,
                    format!(
                        "{} new kills; {} cumulative; {} retained events",
                        s.oom_new.map_or_else(|| "?".into(), |v| v.to_string()),
                        report
                            .account
                            .oom
                            .map_or_else(|| "?".into(), |v| v.to_string()),
                        report.oom.events.len()
                    ),
                    vec![
                        metric("new_kills", s.oom_new.map(|v| v as f64), "count"),
                        metric(
                            "cumulative_kills",
                            report.account.oom.map(|v| v as f64),
                            "count",
                        ),
                    ],
                )
            }
            "io" => {
                let p = &s.processes;
                let sum = p.read_bps.zip(p.write_bps).map(|(r, w)| r + w);
                (
                    level(sum, check),
                    format!(
                        "logical R {}/s W {}/s; {}/{} processes readable{}",
                        p.read_bps
                            .map_or_else(|| "?".into(), |v| util::size(v as u64)),
                        p.write_bps
                            .map_or_else(|| "?".into(), |v| util::size(v as u64)),
                        p.io_read,
                        p.count,
                        if p.io_partial {
                            "; visible-task lower bound"
                        } else {
                            ""
                        }
                    ),
                    vec![
                        metric("read", p.read_bps, "bytes/second"),
                        metric("write", p.write_bps, "bytes/second"),
                    ],
                )
            }
            "threads" | "fds" => {
                let p = &s.processes;
                let (value, read) = if id == "threads" {
                    (p.threads, p.threads_read)
                } else {
                    (p.fds, p.fds_read)
                };
                (
                    level(value.map(|v| v as f64), check),
                    format!(
                        "{} {id}; {read}/{} processes readable",
                        value.map_or_else(|| "?".into(), |v| v.to_string()),
                        p.count
                    ),
                    vec![metric(id, value.map(|v| v as f64), "count")],
                )
            }
            _ => continue,
        };
        results.push(CheckResult::new(id, status, summary, metrics, "builtin"));
    }
    results
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    schema_version: u32,
    status: Status,
    summary: String,
    metrics: Vec<Metric>,
}

fn response(name: &str, data: &[u8]) -> crate::Result<CheckResult> {
    let parsed: Response =
        serde_json::from_slice(data).map_err(|e| format!("invalid check JSON: {e}"))?;
    if parsed.schema_version != 1 {
        return Err("unsupported check schema_version; expected 1".into());
    }
    let mut seen = BTreeSet::new();
    for metric in &parsed.metrics {
        if metric.id.is_empty()
            || metric.id.chars().any(char::is_control)
            || !seen.insert(&metric.id)
            || metric.unit.is_empty()
            || metric.unit.chars().any(char::is_control)
            || metric.value.is_some_and(|v| !v.is_finite())
        {
            return Err("invalid metric id, unit or value".into());
        }
    }
    Ok(CheckResult::new(
        name,
        parsed.status,
        crate::procfs::clean(&parsed.summary),
        parsed.metrics,
        "external",
    ))
}

pub struct External {
    configs: BTreeMap<String, config::Check>,
    running: BTreeSet<String>,
    launched: BTreeMap<String, Instant>,
    results: BTreeMap<String, (CheckResult, Instant)>,
    tx: mpsc::Sender<(String, CheckResult, Instant)>,
    rx: mpsc::Receiver<(String, CheckResult, Instant)>,
    cancelled: Arc<AtomicBool>,
}

impl External {
    pub fn in_flight(&self) -> usize {
        self.running.len()
    }
    pub fn new(config: &config::Config, cancelled: Arc<AtomicBool>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            configs: config
                .checks
                .iter()
                .filter(|(_, c)| c.enabled && !c.command.is_empty())
                .map(|(k, c)| (k.clone(), c.clone()))
                .collect(),
            running: BTreeSet::new(),
            launched: BTreeMap::new(),
            results: BTreeMap::new(),
            tx,
            rx,
            cancelled,
        }
    }
    pub fn complete(&self) -> bool {
        self.results.len() == self.configs.len()
    }
    pub fn wait_budget(&self) -> Duration {
        let seconds = self
            .configs
            .values()
            .map(|c| c.timeout_seconds)
            .sum::<f64>()
            + 0.5;
        Duration::from_secs_f64(seconds.min(60.0))
    }
    pub fn poll(&mut self, report: &Report) -> Vec<CheckResult> {
        while let Ok((name, result, completed)) = self.rx.try_recv() {
            self.running.remove(&name);
            self.results.insert(name, (result, completed));
        }
        if !self.cancelled.load(Ordering::Relaxed) {
            let now = Instant::now();
            let mut due: Vec<_> = self
                .configs
                .iter()
                .filter(|(name, c)| {
                    !self.running.contains(*name)
                        && self.launched.get(*name).is_none_or(|at| {
                            now.duration_since(*at).as_secs_f64() >= c.interval_seconds
                        })
                })
                .collect();
            due.sort_by_key(|(name, _)| self.launched.get(*name));
            for (name, config) in due
                .into_iter()
                .take(2_usize.saturating_sub(self.running.len()))
            {
                self.running.insert(name.clone());
                self.launched.insert(name.clone(), now);
                let name = name.clone();
                let config = config.clone();
                let tx = self.tx.clone();
                let cancelled = self.cancelled.clone();
                let user = report.user.clone();
                let context=serde_json::json!({"schema_version":1,"timestamp":report.timestamp,"host":report.host,"user":report.user,"sample_seconds":report.sample_seconds,"summary":report.summary}).to_string();
                std::thread::spawn(move || {
                    let mut command = std::process::Command::new(&config.command[0]);
                    command.args(&config.command[1..]);
                    command
                        .env("URES_TARGET_USER", user.name)
                        .env("URES_TARGET_UID", user.uid.to_string())
                        .env("URES_CONTEXT", context);
                    let result = util::capture(
                        &mut command,
                        Duration::from_secs_f64(config.timeout_seconds),
                        &cancelled,
                    )
                    .and_then(|data| response(&name, &data))
                    .unwrap_or_else(|e| {
                        CheckResult::new(
                            &name,
                            Status::Error,
                            crate::procfs::clean(&e),
                            Vec::new(),
                            "external",
                        )
                    });
                    let _ = tx.send((name, result, Instant::now()));
                });
            }
        }
        self.configs
            .iter()
            .map(|(name, config)| {
                if let Some((result, at)) = self.results.get(name) {
                    let mut result = result.clone();
                    result.age_seconds = at.elapsed().as_secs_f64();
                    if result.age_seconds > config.interval_seconds + config.timeout_seconds + 5.0 {
                        result.status = Status::Unknown;
                        result.summary = format!("stale: {}", result.summary);
                    }
                    result
                } else {
                    CheckResult::new(
                        name,
                        Status::Unknown,
                        "waiting for first result".into(),
                        Vec::new(),
                        "external",
                    )
                }
            })
            .collect()
    }
}

impl Drop for External {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(1);
        while !self.running.is_empty() && Instant::now() < deadline {
            if let Ok((name, _, _)) = self.rx.recv_timeout(Duration::from_millis(20)) {
                self.running.remove(&name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receiving_a_delayed_result_does_not_reset_its_freshness() {
        let mut config = config::Config::builtin();
        config.checks.insert(
            "delayed".into(),
            config::Check {
                command: vec!["unused".into()],
                interval_seconds: 0.05,
                timeout_seconds: 0.1,
                ..config::Check::default()
            },
        );
        let mut runner = External::new(&config, Arc::new(AtomicBool::new(true)));
        let completed = Instant::now() - Duration::from_secs(10);
        runner
            .tx
            .send((
                "delayed".into(),
                CheckResult::new(
                    "delayed",
                    Status::Ok,
                    "ready".into(),
                    Vec::new(),
                    "external",
                ),
                completed,
            ))
            .unwrap();
        let results = runner.poll(&crate::test_support::report());
        assert_eq!(results[0].status, Status::Unknown);
        assert!(results[0].age_seconds >= 10.0);
        assert!(results[0].summary.starts_with("stale: "));
    }
    #[test]
    fn external_protocol_is_versioned_and_validated() {
        let good=br#"{"schema_version":1,"status":"ok","summary":"ready","metrics":[{"id":"jobs","value":2,"unit":"count"}]}"#;
        assert_eq!(response("queue", good).unwrap().metrics[0].value, Some(2.0));
        let nullable=br#"{"schema_version":1,"status":"unknown","summary":"waiting","metrics":[{"id":"jobs","value":null,"unit":"count"}]}"#;
        assert_eq!(response("queue", nullable).unwrap().metrics[0].value, None);
        for data in [
            br#"{"schema_version":2,"status":"ok","summary":"x","metrics":[]}"#.as_slice(),
            br#"{"schema_version":1,"status":"bogus","summary":"x","metrics":[]}"#.as_slice(),
            br#"{"schema_version":1,"status":"ok","summary":"x","metrics":[{"id":"jobs","unit":"count"}]}"#.as_slice(),
            b"not JSON",
        ] {
            assert!(response("bad", data).is_err());
        }
    }
    #[test]
    fn command_timeout_and_output_limit_are_bounded() {
        let cancel = AtomicBool::new(false);
        let at = Instant::now();
        let result = util::capture(
            std::process::Command::new("sh").args(["-c", "sleep 10"]),
            Duration::from_millis(80),
            &cancel,
        );
        assert!(result.unwrap_err().contains("timed out"));
        assert!(at.elapsed() < Duration::from_secs(2));
        let result = util::capture(
            std::process::Command::new("head").args(["-c", "70000", "/dev/zero"]),
            Duration::from_secs(1),
            &cancel,
        );
        assert!(result.unwrap_err().contains("output exceeded"));
    }
}
