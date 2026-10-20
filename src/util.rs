use crate::Result;
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub static STOP: AtomicBool = AtomicBool::new(false);
pub static SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn stop(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::Relaxed);
    STOP.store(true, Ordering::Relaxed);
}

pub fn install_signals() {
    // SAFETY: handler only sets a lock-free atomic, and has the C signal ABI.
    unsafe {
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
    }
}

pub fn sleep(duration: Duration) {
    let until = std::time::Instant::now() + duration;
    while !STOP.load(Ordering::Relaxed) {
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        std::thread::sleep(left.min(Duration::from_millis(50)));
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub uid: u32,
}

pub fn user(input: Option<&str>) -> Result<User> {
    let numeric = input.and_then(|s| s.parse::<u32>().ok());
    // SAFETY: geteuid has no pointer arguments.
    let requested = numeric.or_else(|| input.is_none().then(|| unsafe { libc::geteuid() }));
    // SAFETY: passwd pointers are checked and copied before another lookup; lookup
    // happens on the main thread before collectors start.
    unsafe {
        let entry = match input {
            Some(name) if numeric.is_none() => libc::getpwnam(
                CString::new(name)
                    .map_err(|_| "invalid user name")?
                    .as_ptr(),
            ),
            _ => libc::getpwuid(requested.unwrap()),
        };
        if !entry.is_null() {
            return Ok(User {
                name: CStr::from_ptr((*entry).pw_name)
                    .to_string_lossy()
                    .into_owned(),
                uid: (*entry).pw_uid,
            });
        }
    }
    // Static musl does not load glibc NSS modules. A host id command can resolve
    // directory-backed names; UID/current-user queries still work without it.
    let cancelled = AtomicBool::new(false);
    let uid = match requested {
        Some(uid) => uid,
        None => capture(
            std::process::Command::new("id").args(["-u", "--", input.unwrap()]),
            Duration::from_secs(2),
            &cancelled,
        )
        .ok()
        .and_then(|v| String::from_utf8_lossy(&v).trim().parse().ok())
        .ok_or_else(|| format!("cannot resolve user {}; try a numeric UID", input.unwrap()))?,
    };
    let name = if let Some(name) = input.filter(|_| numeric.is_none()) {
        name.into()
    } else {
        capture(
            std::process::Command::new("id").args(["-un", "--", &uid.to_string()]),
            Duration::from_secs(2),
            &cancelled,
        )
        .ok()
        .map(|v| crate::procfs::clean(String::from_utf8_lossy(&v).trim()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| uid.to_string())
    };
    Ok(User { name, uid })
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn timestamp(seconds: i64, utc: bool) -> String {
    let raw = seconds;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both conversion functions write a tm to the provided valid pointer.
    let valid = unsafe {
        if utc {
            libc::gmtime_r(&raw, tm.as_mut_ptr())
        } else {
            libc::localtime_r(&raw, tm.as_mut_ptr())
        }
    };
    if valid.is_null() {
        return seconds.to_string();
    }
    // SAFETY: conversion succeeded.
    let tm = unsafe { tm.assume_init() };
    let prefix = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    );
    if utc {
        format!("{prefix}Z")
    } else {
        let offset = tm.tm_gmtoff;
        format!(
            "{prefix}{}{:02}:{:02}",
            if offset < 0 { '-' } else { '+' },
            offset.abs() / 3600,
            offset.abs() % 3600 / 60
        )
    }
}

pub fn duration(s: &str) -> Result<f64> {
    let (number, scale) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1.0),
        Some('m') => (&s[..s.len() - 1], 60.0),
        Some('h') => (&s[..s.len() - 1], 3600.0),
        Some('d') => (&s[..s.len() - 1], 86400.0),
        _ => (s, 1.0),
    };
    let value = number
        .parse::<f64>()
        .map_err(|_| format!("invalid duration: {s}"))?
        * scale;
    if !value.is_finite() || value <= 0.0 || value > 31536000.0 {
        return Err(format!(
            "duration must be positive and at most one year: {s}"
        ));
    }
    Ok(value)
}

pub fn bytes(s: &str) -> Result<u64> {
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let n: f64 = s[..split]
        .parse()
        .map_err(|_| format!("invalid size: {s}"))?;
    let scale = match s[split..].trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kib" | "ki" => 1024.0,
        "mib" | "mi" => 1024.0_f64.powi(2),
        "gib" | "gi" => 1024.0_f64.powi(3),
        "tib" | "ti" => 1024.0_f64.powi(4),
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        _ => return Err(format!("unknown size unit: {s}; use B, MiB, GiB, MB or GB")),
    };
    if !n.is_finite() || n * scale < 1.0 || n * scale >= u64::MAX as f64 {
        return Err(format!("size out of range: {s}"));
    }
    Ok((n * scale) as u64)
}

pub fn size(value: u64) -> String {
    for (scale, unit) in [
        (1_u64 << 40, "TiB"),
        (1 << 30, "GiB"),
        (1 << 20, "MiB"),
        (1 << 10, "KiB"),
    ] {
        if value >= scale {
            return format!("{:.2}{unit}", value as f64 / scale as f64);
        }
    }
    format!("{value}B")
}

/// Capture a monitor-owned process group with bounded output and deadline.
/// Used for optional kernel log backends and explicitly configured checks.
pub fn capture(
    command: &mut std::process::Command,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>> {
    capture_limit(command, timeout, cancelled, 65536)
}

pub fn capture_limit(
    command: &mut std::process::Command,
    timeout: Duration,
    cancelled: &AtomicBool,
    limit: usize,
) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start command: {e}"))?;
    let pid = child.id() as i32;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    // SAFETY: fcntl changes only the monitor's owned pipe handles.
    unsafe {
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            libc::fcntl(
                fd,
                libc::F_SETFL,
                libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK,
            );
        }
    }
    let start = std::time::Instant::now();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let result = (|| {
        let mut exit = None;
        let mut out_eof = false;
        let mut err_eof = false;
        loop {
            if cancelled.load(Ordering::Relaxed) || STOP.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            if start.elapsed() >= timeout {
                return Err(format!("timed out after {:.2}s", timeout.as_secs_f64()));
            }
            for (reader, buffer, eof) in [
                (&mut stdout as &mut dyn Read, &mut out, &mut out_eof),
                (&mut stderr as &mut dyn Read, &mut err, &mut err_eof),
            ] {
                let mut chunk = [0_u8; 4096];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) => {
                            *eof = true;
                            break;
                        }
                        Ok(n) => {
                            buffer.extend_from_slice(&chunk[..n]);
                            if buffer.len() > limit {
                                return Err(format!("output exceeded {limit} bytes"));
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(format!("cannot read command output: {e}")),
                    }
                }
            }
            if exit.is_none() {
                exit = child.try_wait().map_err(|e| e.to_string())?;
            }
            if let Some(status) = exit
                && out_eof
                && err_eof
            {
                if status.success() {
                    return Ok(out);
                }
                return Err(format!(
                    "command exited {}: {}",
                    status,
                    crate::procfs::clean(&String::from_utf8_lossy(&err))
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    // SAFETY: this is the process group created above, never the target user's group.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_are_unambiguous_and_invalid_values_fail() {
        assert_eq!(bytes("8GiB").unwrap(), 8 << 30);
        assert_eq!(bytes("1.5 MB").unwrap(), 1_500_000);
        for s in ["0", "-1GiB", "NaN", "1frogs", "999999999999999999999999TB"] {
            assert!(bytes(s).is_err(), "{s}");
        }
        assert_eq!(duration("24h").unwrap(), 86400.0);
        assert!(duration("NaN").is_err());
    }
    #[test]
    fn utc_timestamp_is_explicit() {
        assert_eq!(timestamp(0, true), "1970-01-01T00:00:00Z");
    }
}
