use std::collections::BTreeMap;
use ures::{
    cgroup,
    model::{Limit, Report, Summary},
    oom,
    procfs::{Process, Totals},
    util,
};

pub fn report() -> Report {
    let process = |pid, ppid, start, cpu, pss, rss, args: &[&str]| Process {
        pid,
        ppid,
        start,
        cpu,
        pss: Some(pss),
        rss,
        state: 'S',
        name: args[0].into(),
        args: args.iter().map(|s| (*s).into()).collect(),
        threads: Some(2),
        fds: Some(8),
        read_bps: Some(1024.0),
        write_bps: Some(512.0),
        ..Process::default()
    };
    let processes: BTreeMap<_, _> = [
        process(1000, 1, 10, 8.0, 16 << 20, 16 << 20, &["sample-shell"]),
        process(
            1001,
            1000,
            20,
            125.0,
            4 << 30,
            (4 << 30) + (256 << 20),
            &[
                "/usr/bin/python3",
                "-B",
                "/opt/ures-demo/bin/sample_worker.py",
                "--config",
                "/opt/ures-demo/config/worker.toml",
            ],
        ),
        process(
            1002,
            1000,
            20,
            10.0,
            1 << 30,
            (1 << 30) + (128 << 20),
            &["/opt/ures-demo/bin/sample-agent", "--job", "example-job"],
        ),
        process(
            2000,
            1,
            10,
            50.0,
            512 << 20,
            576 << 20,
            &[
                "/opt/ures-demo/bin/sample-watcher",
                "--watch",
                "/opt/ures-demo/data",
            ],
        ),
    ]
    .into_iter()
    .map(|p| (p.pid, p))
    .collect();
    let mut total = Totals::default();
    for p in processes.values() {
        total.add(&Totals::own(p));
    }
    let summary = Summary {
        cpu_cores: 1.93,
        cpu_source: "cgroup".into(),
        memory_bytes: 8_396_015_124,
        memory_source: "cgroup charge".into(),
        cpu_limit: Limit {
            value: Some(4.0),
            source: "kernel upper bound".into(),
            accounting: "cgroup".into(),
        },
        memory_limit: Limit {
            value: Some((8_u64 << 30) as f64),
            source: "kernel upper bound".into(),
            accounting: "cgroup charge".into(),
        },
        processes: total,
        throttled_periods: Some(2),
        periods: Some(20),
        throttle_ms: Some(18.5),
        oom_new: Some(0),
    };
    Report {
        schema_version: 1,
        version: ures::VERSION.into(),
        sample_id: 1,
        timestamp: "2030-01-02T03:04:05Z".into(),
        unix_seconds: 1_893_553_445,
        host: "login.example".into(),
        user: util::User {
            name: "alice".into(),
            uid: 1000,
        },
        sample_seconds: 2.0,
        collection_ms: 21.0,
        summary,
        account: cgroup::Stats {
            own_cores: Some(4.0),
            cores: Some(4.0),
            own_limit: Some(8 << 30),
            limit: Some(8 << 30),
            memory: Some(8_396_015_124),
            anon: Some(3 << 30),
            shmem: Some(5 << 29),
            file: Some(8_396_015_124 - (11 << 29)),
            peak: Some(8 << 30),
            oom: Some(3),
            ..cgroup::Stats::default()
        },
        cpu_cgroup: Some("/sys/fs/cgroup/example/user-1000".into()),
        memory_cgroup: Some("/sys/fs/cgroup/example/user-1000".into()),
        processes,
        checks: Vec::new(),
        oom: oom::Log::default(),
        warnings: Vec::new(),
    }
}
