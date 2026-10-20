"""Black-box CLI and PTY checks; standard library only, no live workload changes."""
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import tomllib

BINARY = str(Path(sys.argv[1]).resolve())
PANIC = str(Path(sys.argv[2]).resolve()) if len(sys.argv) > 2 else None

def run(args, env, code=0):
    try:
        result = subprocess.run([BINARY, *args], env=env, text=True, capture_output=True, timeout=30)
    except subprocess.TimeoutExpired:
        raise AssertionError("CLI timed out; live arguments/output withheld") from None
    assert result.returncode == code, f"CLI exit status {result.returncode}; live arguments/output withheld"
    return result.stdout

def terminal_check(env, config, exit_kind, labels=(), extra=(), bare=False):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
    before = termios.tcgetattr(slave)
    def setup():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    command = [PANIC] if exit_kind == "panic" else [BINARY]
    if exit_kind != "panic":
        if not bare:
            command.extend(["--config", str(config), "--sample", "0.1", "-i", "0.2"])
        command.extend(extra)
    child = subprocess.Popen(command,
                             stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=setup)
    output = bytearray()
    try:
        until = time.monotonic() + 8
        marker = b"panic-test" if exit_kind == "panic" else b"CPU"
        # The overview and body may arrive in separate PTY reads/frames.
        while (marker not in output or any(label.encode() not in output for label in labels)) and time.monotonic() < until:
            if select.select([master], [], [], 0.1)[0]:
                output.extend(os.read(master, 65536))
            if child.poll() is not None:
                break
        assert marker in output, "TUI readiness marker missing; live screen withheld"
        for label in labels:
            assert label.encode() in output, "configured tab label missing; live screen withheld"
        if exit_kind != "panic":
            os.write(master, b"?x\t\t\t\t\tcmRR /no-match\r\x1b")
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
            os.kill(child.pid, signal.SIGWINCH)
        # Drain while exiting: a full PTY output buffer can otherwise block the app.
        if exit_kind == "q":
            os.write(master, b"q")
        elif exit_kind == "ctrl-c":
            os.write(master, b"\x03")
        elif exit_kind == "sigterm":
            os.kill(child.pid, signal.SIGTERM)
        until = time.monotonic() + 6
        while child.poll() is None and time.monotonic() < until:
            if select.select([master], [], [], 0.1)[0]:
                output.extend(os.read(master, 65536))
        assert child.poll() is not None, "TUI did not exit promptly"
        expected = {"q": (0,), "ctrl-c": (130,), "sigterm": (143,), "panic": (101,)}[exit_kind]
        assert child.returncode in expected, "unexpected TUI exit status; live screen withheld"
        while select.select([master], [], [], 0.05)[0]:
            output.extend(os.read(master, 65536))
        assert termios.tcgetattr(slave) == before, "terminal state was not restored"
        assert b"\x1b[?1049l" in output, "alternate screen was not restored"
    finally:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        os.close(master)
        os.close(slave)

def terminal_init_failure_check(env):
    if not PANIC:
        return
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
    reader, writer = os.pipe()
    os.close(reader)
    before = termios.tcgetattr(slave)
    def setup():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    try:
        # The example uses the public terminal helper. Its alternate-screen
        # write fails with EPIPE after stdin has entered raw mode.
        child = subprocess.run([PANIC], stdin=slave, stdout=writer, stderr=subprocess.PIPE,
                               env=env, preexec_fn=setup, timeout=8)
        assert child.returncode == 0, "unexpected terminal example exit status"
        assert b"Failed to restore terminal" in child.stderr, "terminal restore error marker missing"
        assert termios.tcgetattr(slave) == before, "failed initialization left terminal raw"
    finally:
        termios.tcsetattr(slave, termios.TCSANOW, before)
        os.close(writer)
        os.close(master)
        os.close(slave)

with tempfile.TemporaryDirectory(prefix="ures-smoke-") as folder:
    root = Path(folder)
    env = dict(os.environ, HOME=str(root), XDG_CONFIG_HOME=str(root / "xdg"), TERM="xterm-256color", NO_COLOR="1")
    for command in ([], ["report"], ["config"]):
        assert "Usage:" in run([*command, "--help"], env)
        brief = run([*command, "-h"], env)
        assert "Usage:" in brief and len(brief.splitlines()) <= 28
        if command != ["config"]:
            assert "Common options:" in brief and "--sample" not in brief
            assert "Selection:" in run([*command, "--help"], env)
        if command:
            assert run(["help", *command], env) == run([*command, "--help"], env)
            assert run(["help", *command, "-h"], env) == brief
        assert run([*command, "-h", "--help"], env) == run([*command, "--help"], env)
    assert run(["-h"], env) == run(["--config", "--help", "-h"], env)
    for args in (["watch"], ["watch", "--help"], ["help", "watch"], ["tui"], ["tui", "--help"], ["help", "tui"], ["report", "--limit", "2"], ["config", "typo", "--help"], ["--unknown", "report", "--help"]):
        run(args, env, 2)
    for command in ("tree", "ps", "groups", "oom", "checks"):
        for args in ([command], [command, "--help"], ["help", command]):
            run(args, env, 2)
    for args in (["config", "views"], ["config", "views", "-h"], ["config", "views", "--help"], ["help", "config", "views"], ["help", "config", "views", "-h"], ["config", "views", "--profile", "work"]):
        run(args, env, 2)
    for command in ([], ["report"], ["config"]):
        assert "ures config views" not in run([*command, "--help"], env)
    for action in ("path", "show", "check", "themes"):
        action_help = run(["config", action, "--help"], env)
        assert action_help == run(["help", "config", action], env)
        assert f"Usage: ures config {action} " in action_help
        brief = run(["config", action, "-h"], env)
        assert brief == run(["help", "config", action, "-h"], env)
        assert len(brief.splitlines()) <= 16
        if action in ("show", "check"):
            assert "--sample" not in brief and "--sample" in action_help
    assert run(["config"], env) == run(["config", "--help"], env)
    assert "then exit" in run(["report", "--help"], env)
    assert run(["help", "--help"], env) == run(["help", "help"], env)
    assert run(["help"], env) == run(["--help"], env)
    assert run(["help", "config", "check"], env) == run(["config", "check", "--help"], env)
    run(["help", "unknown"], env, 2)
    assert [line.split()[0] for line in run(["config", "themes"], env).splitlines()] == ["dracula", "light", "gruvbox"]
    assert not (root / "xdg").exists(), "help created configuration"
    assert run(["--version"], env) == "ures 0.1.0\n"
    for command in ([], ["--view", "io"], ["-i0.1"]):
        result = subprocess.run([BINARY, *command], env=env, text=True, capture_output=True, timeout=10)
        assert result.returncode == 2 and "ures report" in result.stderr, "missing non-terminal mode guidance"
        assert not result.stdout and "\x1b" not in result.stderr, "non-terminal TUI wrote screen data"
    assert not (root / "xdg").exists(), "non-terminal TUI created configuration"
    config = root / "test.toml"
    config.write_text("[checks.oom]\nenabled=false\n")
    assert "Configuration OK" in run(["config", "check", "--config", str(config)], env)
    assert 'default_view = "tree"' in run(["config", "show", "--config", str(config)], env)
    plain = tomllib.loads(run(["config", "show", "--config", str(config)], env))
    assert not plain.get("profiles")
    colored = tomllib.loads(run(["config", "show", "--config", str(config), "--theme", "light"], env))
    assert colored["theme"]["name"] == "light" and colored["theme"]["background"] == "#fffbeb"
    unknown_profile = subprocess.run([BINARY, "config", "check", "--config", str(config), "--profile", "light"], env=env, capture_output=True, text=True, timeout=10)
    assert unknown_profile.returncode == 2 and "--theme light" in unknown_profile.stderr
    invalid_theme = subprocess.run([BINARY, "config", "check", "--config", str(config), "--theme", "missing"], env=env, capture_output=True, text=True, timeout=10)
    assert invalid_theme.returncode == 2 and "unknown theme" in invalid_theme.stderr
    base = ["--config", str(config), "--sample", "0.1"]
    report = json.loads(run(["report", *base, "--json"], env))
    assert report["schema_version"] == 1 and report["user"]["uid"] == os.geteuid()
    assert report["summary"]["cpu_cores"] >= 0
    assert report["summary"]["processes"]["count"] == len(report["processes"])
    # Exercise directory-backed NSS names in addition to numeric UID lookup.
    name = subprocess.check_output(["id", "-un"], text=True).strip()
    assert json.loads(run(["report", *base, "-u", name, "--json"], env))["user"]["uid"] == os.geteuid()
    assert json.loads(run(["report", *base, "-u", str(os.geteuid()), "--json"], env))["user"]["uid"] == os.geteuid()
    for view in ("tree", "ps", "groups", "io"):
        output = run(["report", "--view", view, *base, "--width", "80", "-n", "3"], env)
        assert "CPU" in output and "COMMAND" in output and "\x1b" not in output
    for view in ("ps", "tree"):
        watched = [json.loads(line) for line in run(["report", "--view", view, *base, "--json", "-i0.1", "--count", "2"], env).splitlines()]
        assert [r["sample_id"] for r in watched] == [1, 2]
    text_stream = run(["report", *base, "-i0.1", "--count", "2", "--width", "80"], env)
    assert sum(line.startswith("USER ") for line in text_stream.splitlines()) == 2 and "\x1b" not in text_stream
    for args in (["--json"], ["--full"], ["--width", "80"], ["--count", "2", "-i0.1"], ["report", "--count", "2"], ["config", "--json"], ["config", "path", "--profile", "work"], ["config", "themes", "--config", "unused.toml"], ["config", "show", "--color", "auto"], ["report", "--json", "--color", "auto"]):
        run(args, env, 2)
    profile = root / "profile.toml"
    profile.write_text("[checks.oom]\nenabled=false\n[profiles.lab.limits]\nmode='manual'\ncpu_cores=2.5\nmemory='6GiB'\n")
    manual = json.loads(run(["report", "--config", str(profile), "--profile", "lab", "--sample", "0.1", "--json"], env))
    assert manual["summary"]["cpu_limit"]["value"] == 2.5
    assert manual["summary"]["memory_limit"]["value"] == 6 << 30
    assert manual["summary"]["cpu_limit"]["source"] == "configured budget"
    themed_profile = root / "themed-profile.toml"
    themed_profile.write_text("default_profile='work'\n[theme]\nname='gruvbox'\ncpu='#123456'\n[profiles.work]\ninterval_seconds=3.0\n[profiles.work.theme]\nname='light'\nwrite='#abcdef'\n")
    resolved_theme = tomllib.loads(run(["config", "show", "--config", str(themed_profile), "--theme", "dracula"], env))
    assert resolved_theme["interval_seconds"] == 3.0
    assert resolved_theme["theme"]["name"] == "dracula" and resolved_theme["theme"]["background"] == "#282a36"
    assert resolved_theme["theme"]["cpu"] == "#123456" and resolved_theme["theme"]["write"] == "#abcdef"
    script = root / "literal $(not-a-shell) check.sh"
    script.write_text('#!/bin/sh\nprintf \'{"schema_version":1,"status":"ok","summary":"directory-backed user","metrics":[{"id":"uid","value":%s,"unit":"count"}]}\\n\' "$URES_TARGET_UID"\n')
    plugin = root / "plugin.toml"
    plugin.write_text("[checks.oom]\nenabled=false\n[checks.example]\nenabled=true\ncommand=" + json.dumps(["/bin/sh", str(script)]) + "\n")
    extended = json.loads(run(["report", "--view", "checks", "--config", str(plugin), "--sample", "0.1", "--json"], env))
    check = next(c for c in extended["checks"] if c["id"] == "example")
    assert check["status"] == "ok" and check["metrics"][0]["value"] == os.geteuid()
    invalid = root / "invalid.toml"
    invalid.write_text("[limits]\nmode='manual'\n")
    run(["config", "check", "--config", str(invalid)], env, 2)
    run(["-你"], env, 2)
    run(["tui", "--config", str(config)], env, 2)
    views = root / "views.toml"
    views.write_text("""default_view = 'pipeline'
tab_order = ['pipeline', 'events', 'audit']
[checks.oom]
enabled = false
[views.pipeline]
name = 'Pipeline'
kind = 'tree'
columns = ['pid', 'cpu', 'mem', 'command']
top = 0
[views.events]
name = 'Events'
kind = 'oom'
[views.audit]
name = 'Audit'
kind = 'checks'
""")
    resolved = tomllib.loads(run(["config", "show", "--config", str(views)], env))
    assert set(resolved["views"]) == {"pipeline", "events", "audit"}
    assert resolved["tab_order"] == ["pipeline", "events", "audit"]
    assert resolved["views"]["events"]["name"] == "Events"
    assert "STATUS" in run(["report", "--view", "audit", "--config", str(views), "--sample", "0.1"], env)
    assert "KERNEL PID" in run(["report", "--view", "events", "--config", str(views), "--sample", "0.1", "--since", "1s"], env)
    # Report selection shares the exact configured key set with the TUI.
    assert "COMMAND" in run(["report", "--config", str(views), "--sample", "0.1", "-n1"], env)
    for absent in ("tree", "ps", "groups", "io", "oom", "checks", "Audit", "Pipeline"):
        run(["report", "--view", absent, "--config", str(views)], env, 2)
    health = root / "filtered-checks.toml"
    health.write_text("default_view='health'\n[checks.oom]\nenabled=false\n[views.health]\nkind='checks'\ntop=1\ninclude='DOES_NOT_MATCH_ANY_CHECK'\n")
    output = run(["report", "--config", str(health), "--sample", "0.1"], env)
    section = output.split("STATUS   CHECK             RESULT\n", 1)[1].split("Note:", 1)[0]
    assert not section.strip(), "filtered view unexpectedly contains rows"
    health.write_text(health.read_text().replace("DOES_NOT_MATCH_ANY_CHECK", "cpu|memory"))
    section = run(["report", "--config", str(health), "--sample", "0.1"], env).split("STATUS   CHECK             RESULT\n", 1)[1].split("Note:", 1)[0]
    assert len(section.strip().splitlines()) == 1 and "cpu" in section
    health.write_text(health.read_text().replace("cpu|memory", "configured budget") + "[limits]\nmode='manual'\ncpu_cores=2.0\nmemory='4GiB'\n")
    narrow = run(["report", "--config", str(health), "--sample", "0.1", "--width", "24"], env).splitlines()
    header = next(i for i, line in enumerate(narrow) if line.startswith("STATUS"))
    assert "cpu" in narrow[header + 1], "narrow view omitted the CPU row"
    assert len(narrow[header]) <= 24
    collision = root / "kind-collision.toml"
    collision_base = "default_view='ps'\n[checks.oom]\nenabled=false\n[views.ps]\nkind='tree'\ncolumns=['pid','command']\n"
    collision.write_text(collision_base + "[views.processes]\nkind='ps'\ncolumns=['pid']\n")
    # A key named ps may be a tree; selection never searches by kind.
    output = run(["report", "--view", "processes", "--config", str(collision), "--sample", "0.1", "-n1"], env)
    assert "PID" in output and "COMMAND" not in output
    assert "COMMAND" in run(["report", "--config", str(collision), "--view", "ps", "--sample", "0.1", "-n1"], env)
    # Removing that key removes the CLI view as well, without builtin fallback.
    collision.write_text(collision_base)
    run(["report", "--view", "processes", "--config", str(collision)], env, 2)
    run(["report", "--view", "tree", "--config", str(collision)], env, 2)
    assert "COMMAND" in run(["report", "--config", str(collision), "--sample", "0.1", "-n1"], env)
    terminal_check(env, views, "q", labels=("Pipeline", "Events", "Audit"))
    # This body title identifies the active checks view. Column headers may be
    # drawn as cursor-positioned fragments rather than one contiguous byte run.
    terminal_check(env, views, "q", labels=("builtins and configured extensions",), extra=("--view", "audit"))
    # First normal invocation creates the default; later runs preserve edits.
    run(["report", "--sample", "0.1", "-n1"], env)
    default = root / "xdg/ures/config.toml"
    assert default.exists()
    original = default.read_text()
    default.write_text(original + "\n# user customization\n")
    run(["report", "--sample", "0.1", "-n1"], env)
    assert default.read_text().endswith("# user customization\n")
    terminal_check(env, config, "q", bare=True)
    for method in ("q", "ctrl-c", "sigterm"):
        terminal_check(env, config, method)
    if PANIC:
        terminal_check(env, config, "panic")
    terminal_init_failure_check(env)
    for theme in ("light", "gruvbox"):
        terminal_check(env, config, "q", extra=("--theme", theme, "--color", "always"))
    print("Default TUI, reports/NDJSON, command help, profiles, configured views/tabs, NSS/UID, external checks, config creation, and PTY restoration passed.")
