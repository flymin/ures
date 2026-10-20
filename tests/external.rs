mod support;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, Instant};
use ures::{checks, config, model::Status};

#[test]
fn external_commands_are_limited_to_two_and_each_result_is_retained() {
    let mut config = config::Config::builtin();
    for i in 0..5 {
        config.checks.insert(format!("external{i}"),config::Check {command:vec!["sh".into(),"-c".into(),"sleep 0.05; printf '%s' '{\"schema_version\":1,\"status\":\"ok\",\"summary\":\"ready\",\"metrics\":[]}'".into()],interval_seconds:60.0,..config::Check::default()});
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut runner = checks::External::new(&config, cancelled);
    let report = support::report();
    let at = Instant::now();
    let mut maximum = 0;
    let results = loop {
        let results = runner.poll(&report);
        maximum = maximum.max(runner.in_flight());
        assert!(runner.in_flight() <= 2);
        if runner.complete() {
            break results;
        }
        assert!(at.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(maximum, 2);
    assert_eq!(results.len(), 5);
    assert!(results.iter().all(|r| r.status == Status::Ok));
}

#[test]
fn delayed_poll_preserves_worker_completion_age() {
    let mut config = config::Config::builtin();
    config.checks.insert("age".into(), config::Check {
        command: vec!["sh".into(), "-c".into(), "printf '%s' '{\"schema_version\":1,\"status\":\"ok\",\"summary\":\"ready\",\"metrics\":[]}'".into()],
        interval_seconds: 60.0,
        ..config::Check::default()
    });
    let mut runner = checks::External::new(&config, Arc::new(AtomicBool::new(false)));
    let report = support::report();
    runner.poll(&report);
    std::thread::sleep(Duration::from_millis(500));
    let results = runner.poll(&report);
    assert_eq!(results[0].status, Status::Ok);
    assert!(results[0].age_seconds >= 0.1, "{:?}", results[0]);
}
