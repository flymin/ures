use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub start: u64,
    pub ticks: u64,
    #[serde(rename = "cpu_percent")]
    pub cpu: f64,
    #[serde(rename = "rss_bytes")]
    pub rss: u64,
    #[serde(rename = "pss_bytes")]
    pub pss: Option<u64>,
    pub state: char,
    pub args: Vec<String>,
    pub name: String,
    pub threads: Option<u64>,
    pub fds: Option<u64>,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    pub storage_read_bps: Option<f64>,
    pub storage_write_bps: Option<f64>,
    pub io_partial: bool,
    #[serde(skip)]
    pub io_tasks: BTreeMap<u32, IoCounters>,
}

#[derive(Clone, Debug, Default)]
pub struct IoCounters {
    pub start: u64,
    pub read: u64,
    pub write: u64,
    pub storage_read: u64,
    pub storage_write: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Features {
    pub io: bool,
    pub fds: bool,
    pub threads: bool,
}

impl Process {
    pub fn mem(&self) -> u64 {
        self.pss.unwrap_or(self.rss)
    }

    pub fn group(&self) -> String {
        clean(self.args.first().map_or(self.name.as_str(), |arg| {
            arg.rsplit('/').next().unwrap_or(arg)
        }))
    }

    pub fn command(&self, full: bool) -> String {
        if self.args.is_empty() {
            return format!("[{}]", clean(&self.name));
        }
        let home = std::env::var("HOME").unwrap_or_default();
        self.args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                let text = if !full && (i == 0 || arg.ends_with(".py") || arg.ends_with(".js")) {
                    arg.rsplit('/').next().unwrap_or(arg).to_owned()
                } else if !full && arg.starts_with('/') && arg.len() > 55 {
                    let parts: Vec<_> = arg.rsplit('/').take(3).collect();
                    format!(
                        "…/{}",
                        parts.into_iter().rev().collect::<Vec<_>>().join("/")
                    )
                } else if !full && !home.is_empty() && arg.starts_with(&format!("{home}/")) {
                    format!("~{}", &arg[home.len()..])
                } else {
                    arg.clone()
                };
                clean(&text)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

pub fn parse_stat(text: &str) -> Option<Process> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let fields: Vec<_> = text.get(close + 1..)?.split_whitespace().collect();
    Some(Process {
        pid: text[..open].trim().parse().ok()?,
        name: clean(&text[open + 1..close]),
        state: fields.first()?.chars().next()?,
        ppid: fields.get(1)?.parse().ok()?,
        ticks: fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?,
        start: fields.get(19)?.parse().ok()?,
        ..Process::default()
    })
}

pub struct Snapshot {
    pub at: Instant,
    pub uptime_ticks: u64,
    pub processes: BTreeMap<u32, Process>,
}

pub fn snapshot(uid: u32, detail: bool) -> Snapshot {
    snapshot_with(uid, detail, Features::default())
}

pub fn snapshot_with(uid: u32, detail: bool, features: Features) -> Snapshot {
    let at = Instant::now();
    // SAFETY: sysconf with this constant has no pointer arguments.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    let uptime_ticks = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map_or(0, |seconds| (seconds * hz) as u64);
    let mut processes = BTreeMap::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if pid == std::process::id() {
                continue;
            }
            if let Some(p) = read_process(&entry.path(), uid, detail, features, &mut |path| {
                fs::read(path).ok()
            }) {
                processes.insert(pid, p);
            }
        }
    }
    Snapshot {
        at,
        uptime_ticks,
        processes,
    }
}

fn read_stat(path: &Path, read: &mut impl FnMut(&Path) -> Option<Vec<u8>>) -> Option<Process> {
    parse_stat(&String::from_utf8_lossy(&read(&path.join("stat"))?))
}

fn read_process(
    path: &Path,
    uid: u32,
    detail: bool,
    features: Features,
    read: &mut impl FnMut(&Path) -> Option<Vec<u8>>,
) -> Option<Process> {
    // Establish identity before UID/status, then recheck after collecting the
    // other fields, including the CPU baseline. PID reuse must not mix owners.
    let mut p = read_stat(path, read)?;
    let status = read(&path.join("status"))?;
    // Linux comm is a byte string and may make stat/status invalid UTF-8.
    let status = String::from_utf8_lossy(&status);
    let effective = status.lines().find_map(|line| {
        line.strip_prefix("Uid:")?
            .split_whitespace()
            .nth(1)?
            .parse::<u32>()
            .ok()
    });
    if effective != Some(uid) {
        return None;
    }
    p.rss = kb_field(&status, "VmRSS:").unwrap_or(0);
    if features.threads || features.io {
        p.threads = status
            .lines()
            .find_map(|line| line.strip_prefix("Threads:")?.trim().parse().ok());
    }
    if features.io
        && let Ok(tasks) = fs::read_dir(path.join("task"))
    {
        for task in tasks.flatten() {
            let Ok(tid) = task.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if let Some(io) = read_task_io(&task.path(), read) {
                p.io_tasks.insert(tid, io);
            }
        }
        p.io_partial = p.threads.is_none_or(|n| n as usize != p.io_tasks.len());
    }
    if detail {
        if features.fds {
            p.fds = fs::read_dir(path.join("fd"))
                .ok()
                .map(|entries| entries.flatten().count() as u64);
        }
        p.pss = read(&path.join("smaps_rollup"))
            .and_then(|s| kb_field(&String::from_utf8_lossy(&s), "Pss:"));
        p.args = read(&path.join("cmdline"))
            .unwrap_or_default()
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
    }
    let again = read_stat(path, read)?;
    (again.pid == p.pid && again.start == p.start).then_some(p)
}

fn read_task_io(
    path: &Path,
    read: &mut impl FnMut(&Path) -> Option<Vec<u8>>,
) -> Option<IoCounters> {
    let stat = read_stat(path, read)?;
    let io = read(&path.join("io"))?;
    let again = read_stat(path, read)?;
    if (stat.pid, stat.start) != (again.pid, again.start) {
        return None;
    }
    let io = String::from_utf8_lossy(&io);
    let values: BTreeMap<_, _> = io
        .lines()
        .filter_map(|line| {
            let (k, v) = line.split_once(':')?;
            Some((k, v.trim().parse::<u64>().ok()?))
        })
        .collect();
    Some(IoCounters {
        start: stat.start,
        read: *values.get("rchar")?,
        write: *values.get("wchar")?,
        storage_read: *values.get("read_bytes")?,
        storage_write: *values.get("write_bytes")?,
    })
}

fn kb_field(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
        .map(|v| v * 1024)
}

pub fn cpu_usage(previous: &Snapshot, current: &mut Snapshot) -> f64 {
    let elapsed = current.at.duration_since(previous.at).as_secs_f64();
    // SAFETY: sysconf with this constant has no pointer arguments.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    for p in current.processes.values_mut() {
        let ticks = if let Some(old) = previous
            .processes
            .get(&p.pid)
            .filter(|old| old.start == p.start)
        {
            p.ticks.saturating_sub(old.ticks)
        } else if previous.uptime_ticks > 0 && p.start >= previous.uptime_ticks {
            p.ticks
        } else {
            0
        };
        p.cpu = ticks as f64 / hz / elapsed * 100.0;
        let old_process = previous
            .processes
            .get(&p.pid)
            .filter(|old| old.start == p.start);
        let mut observed = 0;
        let mut totals = [0_u64; 4];
        for (tid, io) in &p.io_tasks {
            let old = old_process
                .and_then(|p| p.io_tasks.get(tid))
                .filter(|old| old.start == io.start);
            if old.is_none() && (previous.uptime_ticks == 0 || io.start < previous.uptime_ticks) {
                p.io_partial = true;
                continue;
            }
            let now = [io.read, io.write, io.storage_read, io.storage_write];
            let before = old.map_or([0; 4], |v| {
                [v.read, v.write, v.storage_read, v.storage_write]
            });
            if now.iter().zip(before).any(|(now, before)| *now < before) {
                p.io_partial = true;
                continue;
            }
            for i in 0..4 {
                totals[i] = totals[i].saturating_add(now[i] - before[i]);
            }
            observed += 1;
        }
        if observed > 0 {
            p.read_bps = Some(totals[0] as f64 / elapsed);
            p.write_bps = Some(totals[1] as f64 / elapsed);
            p.storage_read_bps = Some(totals[2] as f64 / elapsed);
            p.storage_write_bps = Some(totals[3] as f64 / elapsed);
        }
    }
    elapsed
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Totals {
    pub cpu: f64,
    pub mem: u64,
    pub rss: u64,
    pub count: usize,
    pub fallback: usize,
    pub threads: Option<u64>,
    pub fds: Option<u64>,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    pub threads_read: usize,
    pub fds_read: usize,
    pub io_read: usize,
    pub io_partial: bool,
}

impl Totals {
    pub fn own(p: &Process) -> Self {
        Self {
            cpu: p.cpu,
            mem: p.mem(),
            rss: p.rss,
            count: 1,
            fallback: usize::from(p.pss.is_none()),
            threads: p.threads,
            fds: p.fds,
            read_bps: p.read_bps,
            write_bps: p.write_bps,
            threads_read: usize::from(p.threads.is_some()),
            fds_read: usize::from(p.fds.is_some()),
            io_read: usize::from(p.read_bps.is_some()),
            io_partial: p.io_partial,
        }
    }

    pub fn add(&mut self, other: &Self) {
        self.cpu += other.cpu;
        self.mem += other.mem;
        self.rss += other.rss;
        self.count += other.count;
        self.fallback += other.fallback;
        fn sum<T: std::ops::Add<Output = T> + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a + b),
                (a, b) => a.or(b),
            }
        }
        self.threads = sum(self.threads, other.threads);
        self.fds = sum(self.fds, other.fds);
        self.read_bps = sum(self.read_bps, other.read_bps);
        self.write_bps = sum(self.write_bps, other.write_bps);
        self.threads_read += other.threads_read;
        self.fds_read += other.fds_read;
        self.io_read += other.io_read;
        self.io_partial |= other.io_partial;
    }
}

pub fn children(processes: &BTreeMap<u32, Process>) -> BTreeMap<u32, Vec<u32>> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for p in processes.values() {
        if let Some(parent) = parent(p, processes) {
            children.entry(parent).or_default().push(p.pid);
        }
    }
    children
}

pub fn parent(p: &Process, processes: &BTreeMap<u32, Process>) -> Option<u32> {
    processes
        .get(&p.ppid)
        .filter(|parent| parent.pid != p.pid && parent.start <= p.start)
        .map(|parent| parent.pid)
}

pub fn subtree(
    pid: u32,
    processes: &BTreeMap<u32, Process>,
    children: &BTreeMap<u32, Vec<u32>>,
) -> Totals {
    let mut total = Totals::default();
    let mut pending = vec![pid];
    let mut seen = BTreeSet::new();
    while let Some(pid) = pending.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(p) = processes.get(&pid) {
            total.add(&Totals::own(p));
        }
        if let Some(kids) = children.get(&pid) {
            pending.extend(kids);
        }
    }
    total
}

/// Compute every inclusive subtree in linear time; never sum overlapping rows.
pub fn all_totals(processes: &BTreeMap<u32, Process>) -> BTreeMap<u32, Totals> {
    let mut totals: BTreeMap<_, _> = processes
        .iter()
        .map(|(pid, p)| (*pid, Totals::own(p)))
        .collect();
    let mut remaining: BTreeMap<u32, usize> = processes.keys().map(|pid| (*pid, 0)).collect();
    for p in processes.values() {
        if let Some(parent) = parent(p, processes)
            && let Some(n) = remaining.get_mut(&parent)
        {
            *n += 1;
        }
    }
    let mut pending: Vec<_> = remaining
        .iter()
        .filter_map(|(pid, n)| (*n == 0).then_some(*pid))
        .collect();
    while let Some(pid) = pending.pop() {
        let Some(parent) = parent(&processes[&pid], processes) else {
            continue;
        };
        if let Some(n) = remaining.get_mut(&parent) {
            let child = totals[&pid].clone();
            totals.get_mut(&parent).unwrap().add(&child);
            *n -= 1;
            if *n == 0 {
                pending.push(parent);
            }
        }
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_stat(start: u64) -> Vec<u8> {
        let mut fields = vec!["0".to_owned(); 22];
        fields[0] = "S".into();
        fields[1] = "1".into();
        fields[11] = "20".into();
        fields[19] = start.to_string();
        format!("42 (sample) {}", fields.join(" ")).into_bytes()
    }

    fn fixture_reader(
        reads: Vec<(&'static str, Option<Vec<u8>>)>,
    ) -> impl FnMut(&Path) -> Option<Vec<u8>> {
        let mut reads = std::collections::VecDeque::from(reads);
        move |path| {
            let (name, bytes) = reads.pop_front().expect("unexpected fixture read");
            assert_eq!(path.file_name().unwrap(), name);
            bytes
        }
    }

    #[test]
    fn process_identity_brackets_uid_and_data_reads_in_both_modes() {
        for detail in [false, true] {
            for final_stat in [None, Some(fixture_stat(20))] {
                let mut reads = vec![
                    ("stat", Some(fixture_stat(10))),
                    (
                        "status",
                        Some(b"Uid:\t1000 1000 1000 1000\nVmRSS: 8 kB\n".to_vec()),
                    ),
                ];
                if detail {
                    reads.extend([
                        ("smaps_rollup", Some(b"Pss: 4 kB\n".to_vec())),
                        ("cmdline", Some(b"sample-worker\0".to_vec())),
                    ]);
                }
                reads.push(("stat", final_stat));
                let mut read = fixture_reader(reads);
                assert!(
                    read_process(
                        Path::new("/fixture/42"),
                        1000,
                        detail,
                        Features::default(),
                        &mut read
                    )
                    .is_none()
                );
            }
        }
        // A different UID must not trigger expensive detail reads.
        let mut read = fixture_reader(vec![
            ("stat", Some(fixture_stat(10))),
            ("status", Some(b"Uid:\t1001 1001 1001 1001\n".to_vec())),
        ]);
        assert!(
            read_process(
                Path::new("/fixture/42"),
                1000,
                true,
                Features::default(),
                &mut read
            )
            .is_none()
        );
    }

    #[test]
    fn non_utf8_comm_does_not_hide_process_resources() {
        let mut stat = fixture_stat(10);
        let name_end = stat.iter().position(|&byte| byte == b')').unwrap();
        stat.insert(name_end, 0xff);
        for detail in [false, true] {
            let mut reads = vec![
                ("stat", Some(stat.clone())),
                (
                    "status",
                    Some(b"Name:\tsample\xff\nUid:\t1000 1000 1000 1000\nVmRSS: 8 kB\n".to_vec()),
                ),
            ];
            if detail {
                reads.extend([
                    ("smaps_rollup", Some(b"Pss: 4 kB\n".to_vec())),
                    ("cmdline", Some(b"sample-worker\0arg\xff\0".to_vec())),
                ]);
            }
            reads.push(("stat", Some(stat.clone())));
            let mut read = fixture_reader(reads);
            let process = read_process(
                Path::new("/fixture/42"),
                1000,
                detail,
                Features::default(),
                &mut read,
            )
            .unwrap();
            assert_eq!((process.pid, process.start, process.ticks), (42, 10, 20));
            assert_eq!(process.rss, 8 * 1024);
            assert_eq!(process.name, "sample\u{fffd}");
            if detail {
                assert_eq!(process.pss, Some(4 * 1024));
                assert_eq!(process.args, ["sample-worker", "arg\u{fffd}"]);
            }
        }
    }

    #[test]
    fn task_io_rejects_reused_or_disappeared_threads() {
        for (final_stat, accepted) in [
            (None, false),
            (Some(fixture_stat(20)), false),
            (Some(fixture_stat(10)), true),
        ] {
            let mut read = fixture_reader(vec![
                ("stat", Some(fixture_stat(10))),
                (
                    "io",
                    Some(b"rchar: 12\nwchar: 34\nread_bytes: 56\nwrite_bytes: 78\n".to_vec()),
                ),
                ("stat", final_stat),
            ]);
            let counters = read_task_io(Path::new("/fixture/42/task/42"), &mut read);
            assert_eq!(counters.is_some(), accepted);
            if let Some(counters) = counters {
                assert_eq!(
                    (counters.start, counters.read, counters.write),
                    (10, 12, 34)
                );
                assert_eq!((counters.storage_read, counters.storage_write), (56, 78));
            }
        }
    }

    #[test]
    fn parent_pid_reuse_does_not_join_an_unrelated_tree() {
        let p = Process {
            pid: 1,
            ppid: 2,
            start: 10,
            pss: Some(100),
            ..Process::default()
        };
        let new_parent = Process {
            pid: 2,
            ppid: 0,
            start: 20,
            pss: Some(200),
            ..Process::default()
        };
        let processes: BTreeMap<u32, Process> = [(1, p), (2, new_parent)].into();
        assert_eq!(parent(&processes[&1], &processes), None);
        assert_eq!(all_totals(&processes)[&2].mem, 200);
    }
    #[test]
    fn io_uses_task_deltas_and_does_not_import_reaped_child_history() {
        let at = Instant::now();
        let old = Process {
            pid: 1,
            start: 1,
            io_tasks: [(
                1,
                IoCounters {
                    start: 1,
                    read: 100,
                    write: 20,
                    ..IoCounters::default()
                },
            )]
            .into(),
            ..Process::default()
        };
        let new = Process {
            pid: 1,
            start: 1,
            io_tasks: [(
                1,
                IoCounters {
                    start: 1,
                    read: 140,
                    write: 30,
                    ..IoCounters::default()
                },
            )]
            .into(),
            ..Process::default()
        };
        let previous = Snapshot {
            at,
            uptime_ticks: 1000,
            processes: [(1, old)].into(),
        };
        let mut current = Snapshot {
            at: at + std::time::Duration::from_secs(2),
            uptime_ticks: 1200,
            processes: [(1, new)].into(),
        };
        cpu_usage(&previous, &mut current);
        assert_eq!(current.processes[&1].read_bps, Some(20.0));
        assert_eq!(current.processes[&1].write_bps, Some(5.0));
    }
    #[test]
    fn subtree_counts_shared_memory_by_pss_and_keeps_own_separate() {
        let processes: BTreeMap<_, _> = [
            Process {
                pid: 1,
                ppid: 0,
                rss: 100,
                pss: Some(60),
                cpu: 5.0,
                ..Process::default()
            },
            Process {
                pid: 2,
                ppid: 1,
                rss: 100,
                pss: Some(60),
                cpu: 20.0,
                ..Process::default()
            },
            Process {
                pid: 3,
                ppid: 2,
                rss: 40,
                ..Process::default()
            },
        ]
        .into_iter()
        .map(|p| (p.pid, p))
        .collect();
        let c = children(&processes);
        let all = subtree(1, &processes, &c);
        assert_eq!(
            (all.mem, all.rss, all.count, all.fallback),
            (160, 240, 3, 1)
        );
        assert_eq!(all.cpu, 25.0);
        assert_eq!(subtree(2, &processes, &c).mem, 100);
        assert_eq!(processes[&1].mem(), 60);
    }
    #[test]
    fn parses_stat_with_spaces_and_parentheses_in_name() {
        let mut fields = vec!["0"; 22];
        fields[0] = "S";
        fields[1] = "10";
        fields[11] = "70";
        fields[12] = "30";
        fields[19] = "1234";
        let p = parse_stat(&format!("42 (a ) strange (name)) {}", fields.join(" "))).unwrap();
        assert_eq!((p.pid, p.ppid, p.start, p.ticks), (42, 10, 1234, 100));
        assert_eq!(p.name, "a ) strange (name)");
    }
    #[test]
    fn reused_pid_does_not_inherit_old_cpu_time() {
        let at = Instant::now();
        let previous = Snapshot {
            at,
            uptime_ticks: 1000,
            processes: [(
                1,
                Process {
                    pid: 1,
                    start: 1,
                    ticks: 500,
                    ..Process::default()
                },
            )]
            .into(),
        };
        let mut current = Snapshot {
            at: at + std::time::Duration::from_secs(1),
            uptime_ticks: 1100,
            processes: [(
                1,
                Process {
                    pid: 1,
                    start: 1001,
                    ticks: 10,
                    ..Process::default()
                },
            )]
            .into(),
        };
        cpu_usage(&previous, &mut current);
        assert!(current.processes[&1].cpu > 0.0);
        assert!(current.processes[&1].cpu < 100.0);
    }
}
