use crate::{Result, util};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: String,
    pub unix_seconds: i64,
    pub uid: u32,
    pub kernel_pid: u32,
    pub command: String,
    pub rss_bytes: Option<u64>,
    pub shmem_bytes: Option<u64>,
    pub source: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Log {
    pub events: Vec<Event>,
    pub sources: Vec<String>,
    pub warning: Option<String>,
    pub updated_at: String,
}

pub fn iso_seconds(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let fields: Vec<i32> = s
        .get(..19)?
        .split(['-', 'T', ':', ' '])
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    if fields.len() != 6 {
        return None;
    }
    // SAFETY: zeroed tm is valid; timegm normalizes a copied local structure.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = fields[0] - 1900;
    tm.tm_mon = fields[1] - 1;
    tm.tm_mday = fields[2];
    tm.tm_hour = fields[3];
    tm.tm_min = fields[4];
    tm.tm_sec = fields[5];
    // SAFETY: valid mutable tm, not shared with other threads.
    Some(unsafe { libc::timegm(&mut tm) } as i64)
}

pub fn parse(text: &str, uid: u32, source: &str) -> Vec<Event> {
    // Task comm may itself contain parentheses or UID-like text. Kernel fields
    // follow its final closing parenthesis and contain no parentheses.
    let killed = Regex::new(r"Killed process (\d+) \((.*)\)(.*)").unwrap();
    let owner = Regex::new(r"\bUID:(\d+)\b").unwrap();
    // pid/uid are the final kernel context fields; task= may contain lookalikes.
    let context = Regex::new(r",pid=(\d+),uid=(\d+)\s*$").unwrap();
    let stamp = Regex::new(r"\d{4}-\d\d-\d\d[T ]\d\d:\d\d:\d\d").unwrap();
    let resident = Regex::new(r"\b(anon-rss|file-rss|shmem-rss):(\d+)kB").unwrap();
    let mut attributed = None;
    let mut events = Vec::new();
    for line in text.lines() {
        // Match the context record structure, not a short task name such as
        // `oom-kill:` in the following legacy kill record.
        if line.contains("oom-kill:constraint=") {
            attributed = context
                .captures(line)
                .and_then(|c| Some((c[1].parse::<u32>().ok()?, c[2].parse::<u32>().ok()?)));
        }
        let Some(c) = killed.captures(line) else {
            continue;
        };
        let Some(pid) = c[1].parse::<u32>().ok() else {
            continue;
        };
        let event_uid = owner
            .captures(&c[3])
            .and_then(|c| c[1].parse::<u32>().ok())
            .or_else(|| {
                attributed
                    .filter(|(known, _)| *known == pid)
                    .map(|(_, uid)| uid)
            });
        attributed = None;
        if event_uid != Some(uid) {
            continue;
        }
        let Some(timestamp) = stamp.find(line).map(|s| s.as_str()) else {
            continue;
        };
        let Some(unix_seconds) = iso_seconds(timestamp) else {
            continue;
        };
        let mut rss = 0;
        let mut shmem = None;
        let mut present = false;
        for part in resident.captures_iter(&c[3]) {
            let value = part[2].parse::<u64>().unwrap_or(0).saturating_mul(1024);
            rss += value;
            present = true;
            if &part[1] == "shmem-rss" {
                shmem = Some(value);
            }
        }
        events.push(Event {
            timestamp: util::timestamp(unix_seconds, true),
            unix_seconds,
            uid,
            kernel_pid: pid,
            command: crate::procfs::clean(&c[2]),
            rss_bytes: present.then_some(rss),
            shmem_bytes: shmem,
            source: source.into(),
        });
    }
    events
}

pub fn read(uid: u32, cancelled: &AtomicBool) -> Log {
    let mut log = Log {
        updated_at: util::timestamp(util::unix_now(), true),
        ..Log::default()
    };
    let mut failures = Vec::new();
    // dmesg is tried first: it works without systemd in containers.
    let dmesg = util::capture_limit(
        std::process::Command::new("dmesg")
            .args(["--time-format", "iso", "--color=never"])
            .env("TZ", "UTC"),
        Duration::from_secs(2),
        cancelled,
        8 * 1024 * 1024,
    );
    match dmesg {
        Ok(data) => {
            log.events
                .extend(parse(&String::from_utf8_lossy(&data), uid, "dmesg"));
            log.sources.push("dmesg".into());
        }
        Err(e) => failures.push(format!("dmesg: {e}")),
    }
    let journal = util::capture(
        std::process::Command::new("journalctl")
            .args([
                "-k",
                "--no-pager",
                "--utc",
                "-o",
                "short-iso",
                "-g",
                "oom-kill:|Killed process",
                "-n",
                "300",
            ])
            .env("TZ", "UTC"),
        Duration::from_secs(2),
        cancelled,
    );
    match journal {
        Ok(data) if !String::from_utf8_lossy(&data).contains("No entries") => {
            log.events
                .extend(parse(&String::from_utf8_lossy(&data), uid, "journal"));
            log.sources.push("journal".into());
        }
        Ok(_) => {}
        Err(e) => failures.push(format!("journal: {e}")),
    }
    let mut seen = BTreeSet::new();
    log.events.sort_by_key(|e| e.unix_seconds);
    log.events
        .retain(|e| seen.insert((e.timestamp.clone(), e.kernel_pid, e.uid, e.command.clone())));
    if log.sources.is_empty() {
        log.warning = Some(format!(
            "kernel log unavailable; counters may still be readable ({})",
            failures.join("; ")
        ));
    }
    log
}

pub fn since(events: &[Event], seconds: Option<f64>) -> Vec<&Event> {
    let cutoff = seconds.map(|seconds| util::unix_now() - seconds as i64);
    events
        .iter()
        .rev()
        .filter(|e| cutoff.is_none_or(|cutoff| e.unix_seconds >= cutoff))
        .collect()
}

pub fn validate_protocol_timestamp(s: &str) -> Result<()> {
    if iso_seconds(s).is_some() {
        Ok(())
    } else {
        Err("invalid timestamp".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parentheses_and_uid_text_in_comm_cannot_change_event_ownership() {
        for command in ["a ) b", "(worker)", ") UID:1000"] {
            let text = format!(
                "2030-01-02T00:00:00Z Out of memory: Killed process 42 ({command}) total-vm:1024kB, anon-rss:10kB, file-rss:0kB, shmem-rss:2kB, UID:1001 pgtables:1kB oom_score_adj:0"
            );
            assert!(parse(&text, 1000, "fixture").is_empty());
            let events = parse(&text, 1001, "fixture");
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].command, command);
            assert_eq!(events[0].rss_bytes, Some(12 * 1024));
            assert_eq!(events[0].shmem_bytes, Some(2 * 1024));
        }
        let text = "2030-01-02T00:00:00Z oom-kill:constraint=CONSTRAINT_MEMCG,task=pid=43,uid=1000,pid=43,uid=1001\n2030-01-02T00:00:01Z Killed process 43 (pid=43,uid=1000) anon-rss:10kB";
        assert!(parse(text, 1000, "fixture").is_empty());
        let events = parse(text, 1001, "fixture");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].command, "pid=43,uid=1000");
    }
    #[test]
    fn filters_uid_and_does_not_attach_historical_pid_to_current_process() {
        let text = "2030-01-02T03:20:09,707+00:00 Memory cgroup out of memory: Killed process 42 (python) anon-rss:100kB file-rss:10kB shmem-rss:200kB, UID:1000\n2030-01-02T03:20:10+00:00 Killed process 42 (other) anon-rss:999kB UID:1001";
        let events = parse(text, 1000, "fixture");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].rss_bytes, Some(310 * 1024));
        assert_eq!(events[0].shmem_bytes, Some(200 * 1024));
        assert_eq!(events[0].timestamp, "2030-01-02T03:20:09Z");
    }
    #[test]
    fn missing_uid_requires_matching_oom_context() {
        let text = "2030-01-02T03:20:08+00:00 oom-kill:constraint=CONSTRAINT_MEMCG,task=x,pid=42,uid=1000\n2030-01-02T03:20:09+00:00 Killed process 42 (x) anon-rss:1kB\n2030-01-02T03:20:10+00:00 Killed process 43 (y) anon-rss:1kB";
        assert_eq!(parse(text, 1000, "fixture").len(), 1);
        let named = text.replace("(x)", "(oom-kill:)");
        let events = parse(&named, 1000, "fixture");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].command, "oom-kill:");
    }
}
