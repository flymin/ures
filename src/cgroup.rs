use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Mount {
    root: String,
    point: PathBuf,
    controllers: String,
    v2: bool,
}

fn mounts(text: &str) -> Vec<Mount> {
    let escapes = Regex::new(r"\\([0-7]{3})").unwrap();
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let a: Vec<_> = left.split_whitespace().collect();
            let b: Vec<_> = right.split_whitespace().collect();
            if !matches!(*b.first()?, "cgroup" | "cgroup2") {
                return None;
            }
            let decode = |s: &str| {
                escapes
                    .replace_all(s, |c: &regex::Captures<'_>| {
                        char::from(u8::from_str_radix(&c[1], 8).unwrap()).to_string()
                    })
                    .into_owned()
            };
            Some(Mount {
                root: decode(a.get(3)?),
                point: decode(a.get(4)?).into(),
                controllers: b.get(2)?.to_string(),
                v2: b[0] == "cgroup2",
            })
        })
        .collect()
}

fn account_path(path: &str, uid: u32) -> Option<String> {
    let mut end = None;
    for needle in [format!("/login-users/{uid}"), format!("/user-{uid}.slice")] {
        for (offset, _) in path.match_indices(&needle) {
            let candidate = offset + needle.len();
            if (candidate == path.len() || path.as_bytes().get(candidate) == Some(&b'/'))
                && end.is_none_or(|old| candidate < old)
            {
                end = Some(candidate);
            }
        }
    }
    end.map(|end| path[..end].to_owned())
}

fn relative_path(root: &str, group: &str) -> Option<PathBuf> {
    Path::new(group)
        .strip_prefix(root)
        .ok()
        .map(Path::to_path_buf)
}

#[derive(Clone, Debug)]
struct Controller {
    path: PathBuf,
    mount: PathBuf,
    v2: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Account {
    cpu: Option<Controller>,
    memory: Option<Controller>,
    own_cpuset: Option<Controller>,
}

impl Account {
    pub fn discover(
        uid: u32,
        pids: impl Iterator<Item = u32>,
        limits: &crate::config::Limits,
    ) -> Self {
        let mounts = mounts(&fs::read_to_string("/proc/self/mountinfo").unwrap_or_default());
        let mut result = Self::default();
        let self_groups = fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
        let replace_user = Regex::new(r"/login-users/\d+").unwrap();
        let replace_slice = Regex::new(r"/user-\d+\.slice").unwrap();
        let hint = replace_slice
            .replace_all(&self_groups, format!("/user-{uid}.slice"))
            .into_owned();
        let hint = replace_user
            .replace_all(&hint, format!("/login-users/{uid}"))
            .into_owned();
        let sources: Vec<_> = pids
            .filter_map(|pid| fs::read_to_string(format!("/proc/{pid}/cgroup")).ok())
            .collect();
        let normalize = |source: String| {
            source
                .lines()
                .map(|line| {
                    let parts: Vec<_> = line.splitn(3, ':').collect();
                    if parts.len() != 3 {
                        return line.to_owned();
                    }
                    for mount in &mounts {
                        if mount.v2 != parts[1].is_empty()
                            || account_path(&mount.root, uid).is_none()
                            || Path::new(parts[2]).starts_with(&mount.root)
                        {
                            continue;
                        }
                        if !mount.v2
                            && !parts[1]
                                .split(',')
                                .any(|c| mount.controllers.split(',').any(|m| m == c))
                        {
                            continue;
                        }
                        let relative = parts[2].trim_start_matches('/');
                        if mount.point.join(relative).is_dir() {
                            return format!(
                                "{}:{}:{}/{}",
                                parts[0],
                                parts[1],
                                mount.root.trim_end_matches('/'),
                                relative
                            );
                        }
                    }
                    line.to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let sources: Vec<_> = sources.into_iter().map(&normalize).collect();
        let hint = normalize(hint);
        for source in sources.iter().chain(std::iter::once(&hint)) {
            for line in source.lines() {
                let parts: Vec<_> = line.splitn(3, ':').collect();
                if parts.len() != 3 {
                    continue;
                }
                for mount in &mounts {
                    if mount.v2 != parts[1].is_empty() {
                        continue;
                    }
                    if !mount.v2 && !parts[1].split(',').any(|c| c == "cpuset") {
                        continue;
                    }
                    if !mount.v2 && !mount.controllers.split(',').any(|c| c == "cpuset") {
                        continue;
                    }
                    if let Some(group) = account_path(parts[2], uid)
                        && let Some(relative) = relative_path(&mount.root, &group)
                    {
                        let path = mount.point.join(relative);
                        if path.is_dir() && result.own_cpuset.is_none() {
                            result.own_cpuset = Some(Controller {
                                path,
                                mount: mount.point.clone(),
                                v2: mount.v2,
                            });
                        }
                    }
                }
            }
        }
        let mut cpu_groups = std::collections::BTreeSet::new();
        let mut mem_groups = std::collections::BTreeSet::new();
        let mut cpuset_groups = std::collections::BTreeSet::new();
        let mut cpu_unmatched = false;
        let mut mem_unmatched = false;
        let mut cpuset_unmatched = false;
        for source in &sources {
            for line in source.lines() {
                let parts: Vec<_> = line.splitn(3, ':').collect();
                if parts.len() != 3 {
                    continue;
                }
                let cpu = parts[1].is_empty() || parts[1].split(',').any(|s| s == "cpu");
                let mem = parts[1].is_empty() || parts[1].split(',').any(|s| s == "memory");
                let cpuset = parts[1].is_empty() || parts[1].split(',').any(|s| s == "cpuset");
                if let Some(group) = account_path(parts[2], uid) {
                    if cpu {
                        cpu_groups.insert(group.clone());
                    }
                    if mem {
                        mem_groups.insert(group.clone());
                    }
                    if cpuset {
                        cpuset_groups.insert(group);
                    }
                } else {
                    if cpu {
                        cpu_unmatched = true;
                    }
                    if mem {
                        mem_unmatched = true;
                    }
                    if cpuset {
                        cpuset_unmatched = true;
                    }
                }
            }
        }
        for source in sources.iter().chain(std::iter::once(&hint)) {
            for line in source.lines() {
                let parts: Vec<_> = line.splitn(3, ':').collect();
                if parts.len() != 3 {
                    continue;
                }
                let Some(group) = account_path(parts[2], uid) else {
                    continue;
                };
                for mount in &mounts {
                    if mount.v2 != parts[1].is_empty() {
                        continue;
                    }
                    let Some(relative) = relative_path(&mount.root, &group) else {
                        continue;
                    };
                    let path = mount.point.join(relative);
                    if !path.is_dir() {
                        continue;
                    }
                    let controller = Controller {
                        path,
                        mount: mount.point.clone(),
                        v2: mount.v2,
                    };
                    let has = |name: &str| {
                        mount.controllers.split(',').any(|c| c == name)
                            && parts[1].split(',').any(|c| c == name)
                    };
                    if (mount.v2 || has("cpu")) && result.cpu.is_none() {
                        result.cpu = Some(controller.clone());
                    }
                    if (mount.v2 || has("memory")) && result.memory.is_none() {
                        result.memory = Some(controller);
                    }
                }
            }
            if result.cpu.is_some() && result.memory.is_some() {
                break;
            }
        }
        if cpu_groups.len() > 1 || cpu_unmatched {
            result.cpu = None;
        }
        if mem_groups.len() > 1 || mem_unmatched {
            result.memory = None;
        }
        if cpuset_groups.len() > 1 || cpuset_unmatched {
            result.own_cpuset = None;
        }
        for (kind, configured) in [
            ("cpu", &limits.cpu_cgroup),
            ("memory", &limits.memory_cgroup),
        ] {
            if let Some(path) = configured {
                let path = PathBuf::from(path.replace("{uid}", &uid.to_string()));
                let selected = mounts
                    .iter()
                    .filter(|m| {
                        path.starts_with(&m.point)
                            && (m.v2 || m.controllers.split(',').any(|c| c == kind))
                    })
                    .max_by_key(|m| m.point.as_os_str().len())
                    .map(|m| Controller {
                        path: path.clone(),
                        mount: m.point.clone(),
                        v2: m.v2,
                    });
                if kind == "cpu" {
                    // A configured v2 CPU domain also owns its cpuset. Separate
                    // v1 hierarchies cannot be mapped safely to an arbitrary path.
                    if selected.as_ref().is_some_and(|c| c.v2) {
                        result.own_cpuset = selected.clone();
                    } else if result.cpu.as_ref().map(|c| &c.path)
                        != selected.as_ref().map(|c| &c.path)
                    {
                        result.own_cpuset = None;
                    }
                    result.cpu = selected;
                } else {
                    result.memory = selected;
                }
            }
        }
        result
    }

    pub fn paths(&self) -> (Option<String>, Option<String>) {
        (
            self.cpu.as_ref().map(|c| c.path.display().to_string()),
            self.memory.as_ref().map(|c| c.path.display().to_string()),
        )
    }

    pub fn label(&self) -> Option<String> {
        self.memory
            .as_ref()
            .or(self.cpu.as_ref())
            .map(|c| c.path.display().to_string())
    }

    pub fn read(&self) -> Stats {
        let mut result = Stats::default();
        if let Some(c) = &self.cpu {
            result.own_cores = if c.v2 {
                fs::read_to_string(c.path.join("cpu.max"))
                    .ok()
                    .and_then(|text| {
                        let mut p = text.split_whitespace();
                        let q = p.next()?.parse::<f64>().ok()?;
                        let period = p.next()?.parse::<f64>().ok()?;
                        (q > 0.0 && period > 0.0).then_some(q / period)
                    })
            } else {
                fs::read_to_string(c.path.join("cpu.cfs_quota_us"))
                    .ok()
                    .and_then(|q| q.trim().parse::<f64>().ok())
                    .zip(number(&c.path.join("cpu.cfs_period_us")))
                    .and_then(|(q, p)| (q > 0.0 && p > 0).then_some(q / p as f64))
            };
            let stat = values(&c.path.join("cpu.stat"));
            result.cpu_ns = if c.v2 {
                stat.get("usage_usec").map(|v| v * 1000)
            } else {
                number(&c.path.join("cpuacct.usage"))
            };
            result.periods = stat.get("nr_periods").copied();
            result.throttled = stat.get("nr_throttled").copied();
            result.throttle_ns = if c.v2 {
                stat.get("throttled_usec").map(|v| v * 1000)
            } else {
                stat.get("throttled_time").copied()
            };
            result.cores = ancestors(c)
                .filter_map(|path| {
                    if c.v2 {
                        let text = fs::read_to_string(path.join("cpu.max")).ok()?;
                        let mut parts = text.split_whitespace();
                        let quota: f64 = parts.next()?.parse().ok()?;
                        let period: f64 = parts.next()?.parse().ok()?;
                        (quota > 0.0 && period > 0.0).then_some(quota / period)
                    } else {
                        let quota: f64 = fs::read_to_string(path.join("cpu.cfs_quota_us"))
                            .ok()?
                            .trim()
                            .parse()
                            .ok()?;
                        let period: f64 = fs::read_to_string(path.join("cpu.cfs_period_us"))
                            .ok()?
                            .trim()
                            .parse()
                            .ok()?;
                        (quota > 0.0 && period > 0.0).then_some(quota / period)
                    }
                })
                .reduce(f64::min);
        }
        if let Some(c) = &self.own_cpuset {
            // Bound the entire account by its own effective CPUs. A leaf task's
            // cpuset can be much smaller than its siblings' and is not this bound.
            let effective = ["cpuset.cpus.effective", "cpuset.effective_cpus"]
                .iter()
                .find_map(|name| {
                    fs::read_to_string(c.path.join(name))
                        .ok()
                        .and_then(|s| cpu_count(&s))
                })
                .or_else(|| {
                    ancestors(c).find_map(|path| {
                        fs::read_to_string(path.join("cpuset.cpus"))
                            .ok()
                            .and_then(|s| cpu_count(&s))
                    })
                });
            result.cpuset_cpus = effective;
            if let Some(count) = effective {
                result.cores = Some(result.cores.map_or(count as f64, |n| n.min(count as f64)));
            }
            if let Some(count) = fs::read_to_string(c.path.join("cpuset.cpus"))
                .ok()
                .and_then(|s| cpu_count(&s))
            {
                result.own_cores = result.own_cores.or(Some(count as f64));
            }
        }
        if let Some(c) = &self.memory {
            let file = |v1: &str, v2: &str| c.path.join(if c.v2 { v2 } else { v1 });
            result.memory = number(&file("memory.usage_in_bytes", "memory.current"));
            result.own_limit =
                number(&file("memory.limit_in_bytes", "memory.max")).filter(|v| *v < (1 << 60));
            result.peak = number(&file("memory.max_usage_in_bytes", "memory.peak"));
            result.limit = ancestors(c)
                .filter_map(|path| {
                    number(&path.join(if c.v2 {
                        "memory.max"
                    } else {
                        "memory.limit_in_bytes"
                    }))
                })
                .filter(|v| *v < (1 << 60))
                .min();
            let stat = values(&c.path.join("memory.stat"));
            let get = |v1: &str, v2: &str| {
                if c.v2 {
                    stat.get(v2).copied()
                } else {
                    stat.get(&format!("total_{v1}"))
                        .or_else(|| stat.get(v1))
                        .copied()
                }
            };
            result.anon = get("rss", "anon");
            result.shmem = get("shmem", "shmem");
            result.file = get("cache", "file").map(|v| v.saturating_sub(result.shmem.unwrap_or(0)));
            let events = values(&file("memory.oom_control", "memory.events"));
            result.oom = events.get("oom_kill").copied();
            result.hits = if c.v2 {
                events.get("max").copied()
            } else {
                number(&c.path.join("memory.failcnt"))
            };
        }
        result
    }
}

fn ancestors(c: &Controller) -> impl Iterator<Item = &Path> {
    c.path.ancestors().take_while(|p| p.starts_with(&c.mount))
}

fn number(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn values(path: &Path) -> BTreeMap<String, u64> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut p = line.split_whitespace();
            Some((p.next()?.to_owned(), p.next()?.parse().ok()?))
        })
        .collect()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    pub own_cores: Option<f64>,
    pub own_limit: Option<u64>,
    pub cpu_ns: Option<u64>,
    pub cores: Option<f64>,
    pub memory: Option<u64>,
    pub limit: Option<u64>,
    pub peak: Option<u64>,
    pub anon: Option<u64>,
    pub shmem: Option<u64>,
    pub file: Option<u64>,
    pub oom: Option<u64>,
    pub hits: Option<u64>,
    pub periods: Option<u64>,
    pub throttled: Option<u64>,
    pub throttle_ns: Option<u64>,
    pub cpuset_cpus: Option<u64>,
}

fn cpu_count(text: &str) -> Option<u64> {
    if text.trim().is_empty() {
        return None;
    }
    text.trim().split(',').try_fold(0_u64, |total, part| {
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        let a: u64 = a.parse().ok()?;
        let b: u64 = b.parse().ok()?;
        if b < a {
            None
        } else {
            total.checked_add(b - a + 1)
        }
    })
}

pub fn delta(old: Option<u64>, new: Option<u64>) -> Option<u64> {
    new?.checked_sub(old?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_cpuset_does_not_inherit_an_arbitrary_child_task_restriction() {
        for v2 in [false, true] {
            let root =
                std::env::temp_dir().join(format!("ures-cpuset-{}-{v2}", std::process::id()));
            let user = root.join("user");
            let child = user.join("task-a");
            fs::create_dir_all(&child).unwrap();
            let effective = if v2 {
                "cpuset.cpus.effective"
            } else {
                "cpuset.effective_cpus"
            };
            fs::write(user.join("cpuset.cpus"), "0-7").unwrap();
            fs::write(user.join(effective), "0-7").unwrap();
            fs::write(child.join(effective), "0").unwrap();
            if v2 {
                fs::write(user.join("cpu.max"), "400000 100000").unwrap();
            } else {
                fs::write(user.join("cpu.cfs_quota_us"), "400000").unwrap();
                fs::write(user.join("cpu.cfs_period_us"), "100000").unwrap();
            }
            let controller = Controller {
                path: user.clone(),
                mount: root.clone(),
                v2,
            };
            let mut account = Account {
                cpu: Some(controller.clone()),
                own_cpuset: Some(controller),
                ..Account::default()
            };
            let stats = account.read();
            assert_eq!(stats.own_cores, Some(4.0));
            assert_eq!(stats.cores, Some(4.0));
            assert_eq!(stats.cpuset_cpus, Some(8));
            // Empty configured cpus can inherit a smaller effective set.
            fs::write(user.join("cpuset.cpus"), "").unwrap();
            fs::write(user.join(effective), "2-3").unwrap();
            assert_eq!(account.read().cores, Some(2.0));
            // Cpuset-only hierarchies still provide an account upper bound.
            account.cpu = None;
            assert_eq!(account.read().cores, Some(2.0));
            fs::remove_file(user.join(effective)).unwrap();
            fs::write(root.join("cpuset.cpus"), "0-3").unwrap();
            assert_eq!(account.read().cores, Some(4.0));
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn v1_v2_inherited_limits_and_memory_breakdown_match() {
        for v2 in [false, true] {
            let root = std::env::temp_dir().join(format!("ures-cg-{}-{v2}", std::process::id()));
            let user = root.join("user");
            fs::create_dir_all(&user).unwrap();
            let put =
                |path: &Path, name: &str, value: &str| fs::write(path.join(name), value).unwrap();
            if v2 {
                put(&root, "cpu.max", "200000 100000");
                put(&user, "cpu.max", "400000 100000");
                put(&root, "memory.max", "8589934592");
                put(&user, "memory.max", "17179869184");
                put(
                    &user,
                    "cpu.stat",
                    "usage_usec 1500000\nnr_periods 10\nnr_throttled 2\nthrottled_usec 500",
                );
                put(&user, "memory.current", "1073741824");
                put(
                    &user,
                    "memory.stat",
                    "anon 536870912\nfile 134217728\nshmem 67108864",
                );
                put(&user, "memory.events", "oom_kill 5\nmax 9");
            } else {
                for path in [&root, &user] {
                    put(path, "cpu.cfs_period_us", "100000");
                }
                put(&root, "cpu.cfs_quota_us", "200000");
                put(&user, "cpu.cfs_quota_us", "400000");
                put(&user, "cpuacct.usage", "1500000000");
                put(&root, "memory.limit_in_bytes", "8589934592");
                put(&user, "memory.limit_in_bytes", "17179869184");
                put(
                    &user,
                    "cpu.stat",
                    "nr_periods 10\nnr_throttled 2\nthrottled_time 500000",
                );
                put(&user, "memory.usage_in_bytes", "1073741824");
                put(
                    &user,
                    "memory.stat",
                    "total_rss 536870912\ntotal_cache 134217728\ntotal_shmem 67108864",
                );
                put(&user, "memory.oom_control", "oom_kill 5");
                put(&user, "memory.failcnt", "9");
            }
            let c = Controller {
                path: user.clone(),
                mount: root.clone(),
                v2,
            };
            let a = Account {
                cpu: Some(c.clone()),
                memory: Some(c),
                ..Account::default()
            };
            let stats = a.read();
            assert_eq!(stats.own_cores, Some(4.0));
            assert_eq!(stats.cores, Some(2.0));
            assert_eq!(stats.limit, Some(8 << 30));
            assert_eq!(stats.cpu_ns, Some(1_500_000_000));
            assert_eq!(stats.file, Some(64 << 20));
            assert_eq!(stats.oom, Some(5));
            assert_eq!(stats.throttle_ns, Some(500000));
            fs::remove_dir_all(root).unwrap();
        }
        assert_eq!(delta(Some(10), Some(2)), None);
        assert_eq!(cpu_count("0-3,8,10-11"), Some(7));
    }
    #[test]
    fn account_scope_avoids_selecting_another_user_or_the_shared_container() {
        assert_eq!(
            account_path("/pod/login-users/1000/session/abc", 1000).as_deref(),
            Some("/pod/login-users/1000")
        );
        assert_eq!(
            account_path("/user.slice/user-1000.slice/session.scope", 1000).as_deref(),
            Some("/user.slice/user-1000.slice")
        );
        assert_eq!(account_path("/pod/login-users/10000", 1000), None);
        assert_eq!(account_path("/pod", 1000), None);
    }
    #[test]
    fn maps_host_cgroup_paths_into_container_mount() {
        assert_eq!(
            relative_path("/pod/container", "/pod/container/login-users/1000"),
            Some(PathBuf::from("login-users/1000"))
        );
        assert_eq!(relative_path("/pod/container", "/pod/other/1000"), None);
        let mounts = mounts(
            "33 22 0:4 /pod/container /sys/fs/cgroup/cpu,cpuacct ro - cgroup cgroup rw,cpu,cpuacct\n34 22 0:5 / /sys/fs/cgroup rw - cgroup2 cgroup rw",
        );
        assert_eq!(mounts.len(), 2);
        assert!(!mounts[0].v2);
        assert!(mounts[1].v2);
    }
}
