mod support;
#[test]
fn reviewed_tree_layout_snapshot() {
    let report = support::report();
    let config = ures::config::Config::builtin();
    let table = ures::render::table(
        &report,
        &config,
        &config.views["tree"],
        &ures::render::Options::default(),
        110,
    );
    let actual = std::iter::once(table.header)
        .chain(table.lines)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, include_str!("snapshots/tree-110.txt").trim_end());
}
use ures::{config, render};

#[test]
fn non_process_views_filter_before_limits_and_keep_headers() {
    let mut report = support::report();
    report.checks = ["cpu", "memory", "io"]
        .into_iter()
        .map(|id| {
            ures::model::CheckResult::new(
                id,
                ures::model::Status::Ok,
                "ready".into(),
                Vec::new(),
                "builtin",
            )
        })
        .collect();
    let config = config::Config::parse(
        "[views.health]\nkind='checks'\ninclude='memory|io'\nexclude='io'\ntop=1",
        None,
    )
    .unwrap();
    let lines = render::text_view(&report, &config.views["health"], None, "MEMORY", 100, true);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("STATUS"));
    assert!(lines[1].contains("memory"));
    let empty = render::text_view(&report, &config.views["health"], None, "missing", 100, true);
    assert_eq!(empty.len(), 1);
    report.oom.events = ["first", "wanted"]
        .into_iter()
        .map(|command| ures::oom::Event {
            timestamp: report.timestamp.clone(),
            unix_seconds: report.unix_seconds,
            uid: report.user.uid,
            kernel_pid: 99,
            command: command.into(),
            rss_bytes: Some(1024),
            shmem_bytes: None,
            source: "fixture".into(),
        })
        .collect();
    let config =
        config::Config::parse("[views.events]\nkind='oom'\ninclude='wanted'\ntop=1", None).unwrap();
    let lines = render::text_view(&report, &config.views["events"], None, "", 120, true);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("wanted"));
}

#[test]
fn executable_group_labels_cannot_inject_terminal_controls() {
    let mut report = support::report();
    report.processes.get_mut(&2000).unwrap().args = vec!["/bin/danger\u{1b}[31mRED\nrow".into()];
    let config = config::Config::builtin();
    let table = render::table(
        &report,
        &config,
        &config.views["groups"],
        &render::Options::default(),
        120,
    );
    assert!(table.lines.iter().any(|line| line.contains("danger")));
    assert!(
        table
            .lines
            .iter()
            .all(|line| !line.chars().any(char::is_control))
    );
}

#[test]
fn text_view_filters_are_independent_of_terminal_and_field_clipping() {
    let mut report = support::report();
    report.checks = vec![ures::model::CheckResult::new(
        "a-check-with-an-id-beyond-17-columns",
        ures::model::Status::Ok,
        "a very long summary before the final NEEDLE".into(),
        Vec::new(),
        "external",
    )];
    report.oom.events = vec![ures::oom::Event {
        timestamp: report.timestamp.clone(),
        unix_seconds: report.unix_seconds,
        uid: report.user.uid,
        kernel_pid: 99,
        command: "long-executable-name-beyond-18-columns-NEEDLE".into(),
        rss_bytes: None,
        shmem_bytes: None,
        source: "fixture".into(),
    }];
    let config = config::Config::parse(
        "[views.health]\nkind='checks'\ninclude='NEEDLE'\ntop=1\n[views.events]\nkind='oom'\ninclude='NEEDLE'\ntop=1", None,
    ).unwrap();
    let mut empty = report.clone();
    empty.oom.events.clear();
    for width in [0, 1, 20, 24, 40, 120] {
        for (key, search) in [("health", "BEYOND-17"), ("events", "NEEDLE")] {
            let view = &config.views[key];
            let lines = render::text_view(&report, view, None, search, width, false);
            assert_eq!(lines.len(), 2, "{key} width={width}");
            let mut excluded = view.clone();
            excluded.exclude = Some("NEEDLE".into());
            assert_eq!(
                render::text_view(&report, &excluded, None, search, width, false).len(),
                1
            );
            assert!(
                lines
                    .iter()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= width)
            );
        }
        let mut lines = render::check_lines(&report, width, false);
        lines.extend(render::oom_lines(&report, None, 0, width, false));
        lines.extend(render::oom_lines(&empty, None, 0, width, false));
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= width)
        );
    }
}

#[test]
fn unicode_text_fields_keep_results_and_rss_in_the_same_columns() {
    let mut report = support::report();
    let mut positions = Vec::new();
    for name in ["ascii", "中文", "👩‍💻", "لالا"] {
        report.checks = vec![ures::model::CheckResult::new(
            name,
            ures::model::Status::Ok,
            "RESULT-MARKER".into(),
            Vec::new(),
            "external",
        )];
        report.oom.events = vec![ures::oom::Event {
            timestamp: report.timestamp.clone(),
            unix_seconds: report.unix_seconds,
            uid: report.user.uid,
            kernel_pid: 99,
            command: name.into(),
            rss_bytes: Some(1 << 20),
            shmem_bytes: None,
            source: "fixture".into(),
        }];
        let checks = render::check_lines(&report, 120, false);
        let oom = render::oom_lines(&report, None, 0, 120, false);
        let result = checks[1].find("RESULT-MARKER").unwrap();
        let rss = oom[1].find(&ures::util::size(1 << 20)).unwrap();
        positions.push((
            render::cell_width(&checks[1][..result]),
            render::cell_width(&oom[1][..rss]),
        ));
    }
    assert_eq!(positions[0].0, 27);
    assert!(
        positions.windows(2).all(|pair| pair[0] == pair[1]),
        "{positions:?}"
    );
}

#[test]
fn tui_columns_never_depend_on_values_sort_filter_or_empty_results() {
    let report = support::report();
    let mut changed = report.clone();
    changed.processes = report
        .processes
        .values()
        .cloned()
        .map(|mut p| {
            p.pid += 4_000_000;
            if report.processes.contains_key(&p.ppid) {
                p.ppid += 4_000_000;
            }
            p.cpu = 1_234_567.0;
            p.pss = None;
            p.rss = u64::MAX / 8;
            p.read_bps = Some(1e15);
            p.write_bps = Some(0.0);
            p.io_partial = true;
            p.args.push("非常长的 Unicode 命令参数".repeat(20));
            (p.pid, p)
        })
        .collect();
    let config = config::Config::builtin();
    for width in [24, 32, 60, 80, 110, 160] {
        for name in ["tree", "ps", "io", "groups"] {
            let mut view = config.views[name].clone();
            let mut options = render::Options::default();
            let original = render::table_with_layout(
                &report,
                &config,
                &view,
                &options,
                width,
                render::TableLayout::Stable,
            );
            view.sort = "cpu".into();
            for search in ["", "no-such-process-xyz"] {
                options.search = search.into();
                let updated = render::table_with_layout(
                    &changed,
                    &config,
                    &view,
                    &options,
                    width,
                    render::TableLayout::Stable,
                );
                assert_eq!(original.columns, updated.columns, "{name} width={width}");
                assert_eq!(original.header, updated.header, "{name} width={width}");
                for line in &updated.lines {
                    assert!(unicode_width::UnicodeWidthStr::width(line.as_str()) <= width);
                }
            }
        }
    }
}

#[test]
fn tui_reserves_full_pid_and_rates_and_gives_commands_the_remaining_space() {
    let mut report = support::report();
    let mut process = report.processes.remove(&2000).unwrap();
    process.pid = 4_194_304;
    process.read_bps = Some(1023.0 * (1_u64 << 30) as f64);
    process.io_partial = true;
    process.args = vec!["long-command ".repeat(40)];
    report.processes.insert(process.pid, process);
    let config = config::Config::builtin();
    let table = render::table_with_layout(
        &report,
        &config,
        &config.views["io"],
        &render::Options::default(),
        110,
        render::TableLayout::Stable,
    );
    let line = table
        .rows
        .iter()
        .zip(&table.lines)
        .find(|(r, _)| r.pid == Some(4_194_304))
        .unwrap()
        .1;
    assert!(line.contains("4194304"));
    assert!(line.contains("1023.00GiB~"));
    assert!(line.ends_with('…'));
    assert_eq!(
        table.columns.iter().map(|(_, w)| w).sum::<usize>() + (table.columns.len() - 1) * 2,
        110
    );
    let config = config::Config::parse(
        "[columns.read]\nwidth=14\n[columns.command]\nmax_width=20",
        None,
    )
    .unwrap();
    let table = render::table_with_layout(
        &report,
        &config,
        &config.views["io"],
        &render::Options::default(),
        160,
        render::TableLayout::Stable,
    );
    assert!(table.columns.contains(&("read".into(), 14)));
    assert!(table.columns.contains(&("command".into(), 20)));
}

#[test]
fn configured_minima_override_implicit_caps_when_the_viewport_has_space() {
    let mut report = support::report();
    let mut config = config::Config::parse("[views.ps]\ncolumns=['pid','command']\n[columns.pid]\nmin_width=24\n[columns.command]\nmin_width=96", None).unwrap();
    for layout in [render::TableLayout::Compact, render::TableLayout::Stable] {
        let table = render::table_with_layout(
            &report,
            &config,
            &config.views["ps"],
            &render::Options::default(),
            200,
            layout,
        );
        assert!(
            table
                .columns
                .iter()
                .any(|(key, width)| key == "pid" && *width >= 24)
        );
        assert!(
            table
                .columns
                .iter()
                .any(|(key, width)| key == "command" && *width >= 96)
        );
        assert_eq!(table.header.find("COMMAND"), Some(26));
    }
    // An explicit maximum continues to cap long content above the minimum.
    config.columns.get_mut("command").unwrap().max_width = Some(104);
    report.processes.get_mut(&2000).unwrap().args = vec!["x".repeat(200)];
    let table = render::table(
        &report,
        &config,
        &config.views["ps"],
        &render::Options::default(),
        200,
    );
    assert!(table.columns.contains(&("command".into(), 104)));
    // Impossible minima are clipped without allocating their requested size.
    config.columns.get_mut("pid").unwrap().min_width = usize::MAX;
    let table = render::table(
        &report,
        &config,
        &config.views["ps"],
        &render::Options::default(),
        40,
    );
    assert!(table.columns.iter().all(|(_, width)| *width <= 40));
    assert!(
        table
            .lines
            .iter()
            .all(|line| render::cell_width(line) <= 40)
    );
}

#[test]
fn tree_totals_and_collapsed_rows_keep_the_same_memory_accounting() {
    let report = support::report();
    let config = config::Config::builtin();
    let view = &config.views["tree"];
    let mut options = render::Options::default();
    let rows = render::rows(&report, &config, view, &options);
    assert_eq!(rows[0].pid, Some(1000));
    assert_eq!(rows[0].totals.mem, (5 << 30) + (16 << 20));
    assert_eq!(rows[0].own_mem, 16 << 20);
    assert_eq!(rows[1].pid, Some(1001));
    assert_eq!(rows[2].pid, Some(1002));
    options.collapsed.insert(1000);
    let folded = render::rows(&report, &config, view, &options);
    assert_eq!(folded.len(), 2);
    assert_eq!(folded[0].totals.mem, rows[0].totals.mem);
    assert!(folded[0].command.contains("[+]"));
}

#[test]
fn filters_keep_context_and_application_groups_do_not_double_count() {
    let report = support::report();
    let mut config = config::Config::builtin();
    let mut view = config.views["tree"].clone();
    view.include = Some("sample_worker".into());
    let rows = render::rows(&report, &config, &view, &render::Options::default());
    assert_eq!(rows.len(), 2);
    assert!(rows[0].context);
    assert_eq!(rows[1].pid, Some(1001));
    config.groups.push(config::Group {
        name: "Interactive".into(),
        pattern: "sample-shell|sample-agent".into(),
    });
    let grouped = render::rows(
        &report,
        &config,
        &config.views["groups"],
        &render::Options::default(),
    );
    assert_eq!(
        grouped.iter().map(|r| r.totals.mem).sum::<u64>(),
        report.summary.processes.mem
    );
    assert_eq!(
        grouped
            .iter()
            .find(|r| r.command == "Interactive")
            .unwrap()
            .totals
            .count,
        2
    );
}

#[test]
fn layout_is_bounded_at_narrow_and_wide_widths_and_json_roundtrips() {
    let report = support::report();
    let config = config::Config::builtin();
    for width in [32, 60, 80, 120, 160] {
        let table = render::table(
            &report,
            &config,
            &config.views["tree"],
            &render::Options::default(),
            width,
        );
        for line in std::iter::once(&table.header).chain(&table.lines) {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= width,
                "{width}: {line}"
            );
        }
    }
    let json = serde_json::to_string(&report).unwrap();
    let decoded: ures::model::Report = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.processes[&1001].pss, Some(4 << 30));
    assert_eq!(decoded.summary.cpu_limit.value, Some(4.0));
}
