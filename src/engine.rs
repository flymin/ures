use crate::{
    Result, cgroup, checks, config,
    model::{Limit, Report, Summary},
    oom, procfs, util,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

pub fn features(config: &config::Config, view: &config::View) -> procfs::Features {
    let enabled = |id: &str| {
        config
            .checks
            .get(id)
            .is_some_and(|c| c.enabled && c.command.is_empty())
    };
    procfs::Features {
        io: enabled("io")
            || view.sort == "io"
            || view.columns.iter().any(|s| s == "read" || s == "write"),
        threads: enabled("threads")
            || view.sort == "threads"
            || view.columns.iter().any(|s| s == "threads"),
        fds: enabled("fds") || view.sort == "fds" || view.columns.iter().any(|s| s == "fds"),
    }
}

pub fn needs_oom<'a>(
    config: &config::Config,
    mut views: impl Iterator<Item = &'a config::View>,
) -> bool {
    config.checks.get("oom").is_some_and(|c| c.enabled) || views.any(|view| view.kind == "oom")
}

pub struct Engine {
    user: util::User,
    config: config::Config,
    pub features: procfs::Features,
    previous: procfs::Snapshot,
    previous_account: cgroup::Stats,
    previous_paths: (Option<String>, Option<String>),
    previous_cg_at: Instant,
    log: oom::Log,
    log_at: Option<Instant>,
    need_oom: bool,
    external: checks::External,
    cancelled: Arc<AtomicBool>,
    sequence: u64,
}

impl Engine {
    pub fn new(
        user: util::User,
        config: config::Config,
        features: procfs::Features,
        need_oom: bool,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        for path in [&config.limits.cpu_cgroup, &config.limits.memory_cgroup]
            .into_iter()
            .flatten()
        {
            let path = path.replace("{uid}", &user.uid.to_string());
            if !std::path::Path::new(&path).is_dir() {
                return Err(format!(
                    "configured cgroup directory does not exist: {path}"
                ));
            }
        }
        let previous = procfs::snapshot_with(user.uid, false, features);
        let account =
            cgroup::Account::discover(user.uid, previous.processes.keys().copied(), &config.limits);
        let previous_account = account.read();
        let previous_paths = account.paths();
        let external = checks::External::new(&config, cancelled.clone());
        Ok(Self {
            user,
            config,
            features,
            previous,
            previous_account,
            previous_paths,
            previous_cg_at: Instant::now(),
            log: oom::Log::default(),
            log_at: None,
            need_oom,
            external,
            cancelled,
            sequence: 0,
        })
    }

    pub fn sample(&mut self) -> Report {
        let started = Instant::now();
        let sample_time = util::unix_now();
        self.sequence += 1;
        let mut current = procfs::snapshot_with(self.user.uid, true, self.features);
        let sample_seconds = procfs::cpu_usage(&self.previous, &mut current);
        let account = cgroup::Account::discover(
            self.user.uid,
            current.processes.keys().copied(),
            &self.config.limits,
        );
        let at = Instant::now();
        let stats = account.read();
        let paths = account.paths();
        let cg_seconds = at.duration_since(self.previous_cg_at).as_secs_f64();
        let same_cpu = paths.0.is_some() && paths.0 == self.previous_paths.0;
        let same_mem = paths.1.is_some() && paths.1 == self.previous_paths.1;
        let old = if same_cpu {
            self.previous_account.cpu_ns
        } else {
            None
        };
        let account_cpu = cgroup::delta(old, stats.cpu_ns).map(|n| n as f64 / 1e9 / cg_seconds);
        let mut total = procfs::Totals::default();
        for p in current.processes.values() {
            total.add(&procfs::Totals::own(p));
        }
        let (cpu_cores, cpu_source) = account_cpu
            .map_or((total.cpu / 100.0, "visible processes".to_owned()), |v| {
                (v, "cgroup".into())
            });
        let (memory_bytes, memory_source) = stats
            .memory
            .map_or((total.mem, "visible process PSS/RSS".to_owned()), |v| {
                (v, "cgroup charge".into())
            });
        let select = |detected: Option<f64>, manual: Option<f64>, accounting: &str| {
            let selected = match self.config.limits.mode.as_str() {
                "manual" => manual.map(|v| (v, "configured budget")),
                "auto-or-manual" => detected
                    .map(|v| (v, "kernel upper bound"))
                    .or(manual.map(|v| (v, "configured budget"))),
                _ => detected.map(|v| (v, "kernel upper bound")),
            };
            selected.map_or(
                Limit {
                    value: None,
                    source: "not detected".into(),
                    accounting: accounting.into(),
                },
                |(value, source)| Limit {
                    value: Some(value),
                    source: source.into(),
                    accounting: accounting.into(),
                },
            )
        };
        let summary = Summary {
            cpu_cores,
            cpu_source: cpu_source.clone(),
            memory_bytes,
            memory_source: memory_source.clone(),
            cpu_limit: select(
                stats.own_cores.and(stats.cores),
                self.config.limits.cpu_cores,
                &cpu_source,
            ),
            memory_limit: select(
                stats.own_limit.and(stats.limit).map(|v| v as f64),
                self.config
                    .limits
                    .memory
                    .as_deref()
                    .and_then(|s| util::bytes(s).ok())
                    .map(|v| v as f64),
                &memory_source,
            ),
            processes: total,
            throttled_periods: if same_cpu {
                cgroup::delta(self.previous_account.throttled, stats.throttled)
            } else {
                None
            },
            periods: if same_cpu {
                cgroup::delta(self.previous_account.periods, stats.periods)
            } else {
                None
            },
            throttle_ms: if same_cpu {
                cgroup::delta(self.previous_account.throttle_ns, stats.throttle_ns)
                    .map(|n| n as f64 / 1e6)
            } else {
                None
            },
            oom_new: if same_mem {
                cgroup::delta(self.previous_account.oom, stats.oom)
            } else {
                None
            },
        };
        if self.need_oom
            && (self
                .log_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(10))
                || summary.oom_new.unwrap_or(0) > 0)
        {
            self.log = oom::read(self.user.uid, &self.cancelled);
            self.log_at = Some(Instant::now());
        }
        let mut warnings = Vec::new();
        if paths.0.is_none() || paths.1.is_none() {
            warnings.push("No single user cgroup covers this process set; shared host/container limits are not a personal quota. Configure a profile if needed.".into());
        }
        if summary.processes.fallback > 0 {
            warnings.push(format!("PSS unavailable for {}/{} processes; * marks RSS fallback. Process totals exclude unmapped cgroup cache.",summary.processes.fallback,summary.processes.count));
        }
        if self.features.io {
            warnings.push("I/O rates cover observed live tasks, not a complete disk/NFS measurement; exited tasks may leave an unobserved tail.".into());
        }
        if let Some(warning) = &self.log.warning {
            warnings.push(warning.clone());
        }
        let mut report = Report {
            schema_version: 1,
            version: crate::VERSION.into(),
            sample_id: self.sequence,
            timestamp: util::timestamp(sample_time, true),
            unix_seconds: sample_time,
            host: std::fs::read_to_string("/proc/sys/kernel/hostname")
                .unwrap_or_else(|_| "localhost".into())
                .trim()
                .into(),
            user: self.user.clone(),
            sample_seconds,
            collection_ms: started.elapsed().as_secs_f64() * 1000.0,
            summary,
            account: stats.clone(),
            cpu_cgroup: paths.0.clone(),
            memory_cgroup: paths.1.clone(),
            processes: current.processes.clone(),
            checks: Vec::new(),
            oom: self.log.clone(),
            warnings,
        };
        report.checks = checks::builtins(&self.config, &report);
        let external = self.external.poll(&report);
        report.checks.extend(external);
        self.previous = current;
        self.previous_account = stats;
        self.previous_paths = paths;
        self.previous_cg_at = at;
        report
    }

    pub fn wait_checks(&mut self, report: &mut Report) {
        let deadline = Instant::now() + self.external.wait_budget();
        while !self.external.complete()
            && Instant::now() < deadline
            && !util::STOP.load(Ordering::Relaxed)
        {
            util::sleep(Duration::from_millis(20));
            let external = self.external.poll(report);
            report.checks.retain(|c| c.source != "external");
            report.checks.extend(external);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub enum Control {
    Refresh,
    Features(procfs::Features),
    Stop,
}
pub type Latest = Arc<Mutex<Option<std::result::Result<Report, String>>>>;
pub struct Worker {
    pub latest: Latest,
    pub tx: mpsc::Sender<Control>,
    cancelled: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn spawn(user: util::User, config: config::Config, features: procfs::Features) -> Self {
        let latest: Latest = Arc::new(Mutex::new(None));
        let state = latest.clone();
        let (tx, rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let handle = std::thread::spawn(move || {
            let need_oom = needs_oom(&config, config.views.values());
            let mut engine =
                match Engine::new(user, config.clone(), features, need_oom, cancel.clone()) {
                    Ok(e) => e,
                    Err(e) => {
                        *state.lock().unwrap() = Some(Err(e));
                        return;
                    }
                };
            let mut due = Instant::now() + Duration::from_secs_f64(config.sample_seconds);
            while !cancel.load(Ordering::Relaxed) && !util::STOP.load(Ordering::Relaxed) {
                match rx.recv_timeout(
                    due.saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(100)),
                ) {
                    Ok(Control::Stop) => break,
                    Ok(Control::Features(features)) => {
                        engine.features = features;
                    }
                    Ok(Control::Refresh) => {
                        due = due.min(Instant::now() + Duration::from_millis(50))
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    _ => {}
                }
                if Instant::now() >= due {
                    let report = engine.sample();
                    *state.lock().unwrap() = Some(Ok(report));
                    due = Instant::now() + Duration::from_secs_f64(config.interval_seconds);
                }
            }
        });
        Self {
            latest,
            tx,
            cancelled,
            handle: Some(handle),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Control::Stop);
        if let Some(handle) = self.handle.take() {
            let deadline = Instant::now() + Duration::from_secs(1);
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oom_logs_follow_checks_and_available_views() {
        let config = config::Config::parse(
            "[checks.oom]\nenabled=false\n[views.work]\nkind='tree'\ncolumns=['pid','command']",
            None,
        )
        .unwrap();
        assert!(!needs_oom(&config, config.views.values()));
        let mut with_history = config.clone();
        let mut history = config.views["work"].clone();
        history.kind = "oom".into();
        history.columns.clear();
        with_history.views.insert("history".into(), history);
        // A hidden tab still needs logs, while a report of only work does not.
        assert!(needs_oom(&with_history, with_history.views.values()));
        assert!(!needs_oom(
            &with_history,
            std::iter::once(&with_history.views["work"])
        ));
        assert!(needs_oom(
            &with_history,
            std::iter::once(&with_history.views["history"])
        ));
        let mut with_check = config;
        with_check.checks.get_mut("oom").unwrap().enabled = true;
        assert!(needs_oom(&with_check, with_check.views.values()));
    }
}
