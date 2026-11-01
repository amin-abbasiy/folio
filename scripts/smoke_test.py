#!/usr/bin/env python3
"""Real-window end-to-end test. Run inside xvfb-run; never uses your normal notebook."""

import contextlib
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "target/debug/folio").resolve()
ARTIFACTS = Path(tempfile.mkdtemp(prefix="folio-ui-"))
DATA = ARTIFACTS / "notebook"
ENV = dict(os.environ, LIBGL_ALWAYS_SOFTWARE="1")
ENV["XDG_RUNTIME_DIR"] = str(ARTIFACTS / "runtime")
Path(ENV["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
LOG = (ARTIFACTS / "application.log").open("w")
WM_LOG = (ARTIFACTS / "window-manager.log").open("w")
process = None
window = None
wm = None
traces = []


def run(*args, check=True):
    return subprocess.run(args, env=ENV, check=check, capture_output=True, text=True)


def wait_for(predicate, message, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        if process and process.poll() is not None:
            raise AssertionError(f"App exited early: {process.returncode}; see {ARTIFACTS}")
        time.sleep(0.08)
    raise AssertionError(f"Timed out: {message}; see {ARTIFACTS}")


def query(sql, parameters=()):
    if not (DATA / "notes.sqlite3").exists():
        return []
    with contextlib.closing(sqlite3.connect(f"file:{DATA / 'notes.sqlite3'}?mode=ro", uri=True)) as connection:
        return connection.execute(sql, parameters).fetchall()


def keys(sequence, pause=0.15):
    run("xdotool", "key", "--clearmodifiers", sequence)
    time.sleep(pause)


def type_text(text):
    # Return must be a key event, not an injected control character. Let the final
    # frame consume keyboard events before moving pointer focus to another widget.
    for index, line in enumerate(text.split("\n")):
        if index:
            keys("Return", pause=0.05)
        if line:
            run("xdotool", "type", "--clearmodifiers", "--delay", "4", "--", line)
            time.sleep(0.12)
    time.sleep(0.12)


def click(x, y):
    run("xdotool", "mousemove", "--window", window, str(x), str(y), "click", "1")
    time.sleep(0.15)


def screenshot(name):
    run("xdotool", "mousemove", "--window", window, "1", "1")
    time.sleep(0.25)
    run("import", "-display", ENV["DISPLAY"], "-window", window, str(ARTIFACTS / name))


def launch():
    global process, window
    command = [str(BINARY), "--data-dir", str(DATA)]
    if shutil.which("strace"):
        trace = ARTIFACTS / f"network-{len(traces) + 1}.log"
        traces.append(trace)
        command = ["strace", "-f", "-e", "trace=network", "-o", str(trace), *command]
    process = subprocess.Popen(command, env=ENV, stdout=LOG, stderr=LOG)

    def find_window():
        global window
        # Never select a pre-existing Folio window, even if this script is invoked
        # on the wrong display. With strace, the app is the tracer's direct child.
        pids = [str(process.pid)]
        children = Path(f"/proc/{process.pid}/task/{process.pid}/children")
        if children.exists():
            pids.extend(children.read_text().split())
        for pid in pids:
            result = run("xdotool", "search", "--all", "--onlyvisible", "--pid", pid, "--name", "^Folio$", check=False)
            if result.returncode == 0 and result.stdout.strip():
                window = result.stdout.strip().splitlines()[-1]
                return True
        return False

    wait_for(find_window, "Folio window", timeout=20)
    run("xdotool", "windowactivate", "--sync", window)
    time.sleep(0.5)


def close():
    keys("alt+F4", pause=0.05)
    process.wait(timeout=10)
    assert process.returncode == 0, f"Unclean exit: {process.returncode}"


try:
    assert "DISPLAY" in ENV, "Run this script with xvfb-run, not on a real desktop."
    wm = subprocess.Popen(["openbox", "--sm-disable"], env=ENV, stdout=WM_LOG, stderr=WM_LOG)
    time.sleep(0.4)
    launch()
    assert query("SELECT count(*) FROM notes")[0][0] == 3
    screenshot("welcome.png")
    (ROOT / "docs").mkdir(exist_ok=True)
    shutil.copyfile(ARTIFACTS / "welcome.png", ROOT / "docs/screenshot.png")

    click(1063, 39)
    wait_for(lambda: query("SELECT value FROM settings WHERE key = 'theme'") == [("dark",)], "dark appearance")
    screenshot("welcome-dark.png")
    shutil.copyfile(ARTIFACTS / "welcome-dark.png", ROOT / "docs/screenshot-dark.png")
    click(1063, 39)
    wait_for(lambda: query("SELECT value FROM settings WHERE key = 'theme'") == [("light",)], "light appearance")

    duplicate = run(str(BINARY), "--data-dir", str(DATA), check=False)
    assert duplicate.returncode != 0 and "already open" in duplicate.stderr
    print("PASS: startup, first-run content, single-instance lock", flush=True)

    keys("ctrl+n")
    type_text("A thought from the desktop test")
    click(650, 345)
    content = "## A saved thought\n\n- [ ] First task\n- [x] Second task\n\n**Keep this private.**"
    type_text(content)
    keys("ctrl+Return")
    wait_for(lambda: bool(query("SELECT id FROM notes WHERE title = ? AND body = ?", ("A thought from the desktop test", content))), "autosave of title and body")
    note_id = query("SELECT id FROM notes WHERE title = ?", ("A thought from the desktop test",))[0][0]
    screenshot("note-preview.png")
    click(550, 347)
    content = content.replace("- [ ] First task", "- [x] First task")
    wait_for(lambda: query("SELECT body FROM notes WHERE id = ?", (note_id,)) == [(content,)], "interactive checklist save")
    print("PASS: real keyboard input, Markdown preview, autosave", flush=True)

    keys("ctrl+shift+p")
    wait_for(lambda: query("SELECT pinned FROM notes WHERE id = ?", (note_id,)) == [(1,)], "pin shortcut")

    keys("ctrl+shift+e")
    exported = ARTIFACTS / "exported.md"
    keys("ctrl+a")
    type_text(str(exported))
    keys("Return")
    wait_for(exported.exists, "Markdown export")
    assert "Keep this private." in exported.read_text()

    keys("ctrl+shift+b")
    backup = ARTIFACTS / "manual-backup.sqlite3"
    keys("ctrl+a")
    type_text(str(backup))
    keys("Return")
    wait_for(backup.exists, "backup")
    with contextlib.closing(sqlite3.connect(backup)) as connection:
        assert connection.execute("PRAGMA quick_check").fetchone() == ("ok",)
        assert connection.execute("SELECT body FROM notes WHERE id = ?", (note_id,)).fetchone() == (content,)
    print("PASS: pinning, local export, consistent notebook backup", flush=True)

    click(1194, 103)
    wait_for(lambda: query("SELECT deleted_at IS NOT NULL FROM notes WHERE id = ?", (note_id,)) == [(1,)], "moving a note to trash")
    click(70, 225)
    click(1194, 103)
    wait_for(lambda: query("SELECT deleted_at FROM notes WHERE id = ?", (note_id,)) == [(None,)], "restoring a note")
    click(70, 137)
    print("PASS: recoverable trash and restore through the interface", flush=True)

    imported = ARTIFACTS / "import-me.md"
    imported.write_text("# Imported from disk\n\nA new local thought.\n\n![Never loaded](https://example.com/tracker.png)\n\n<script>never_executed()</script>\n")
    keys("ctrl+o")
    keys("ctrl+a")
    type_text(str(imported))
    keys("Return")
    wait_for(lambda: bool(query("SELECT id FROM notes WHERE title = 'Imported from disk'")), "Markdown import")

    keys("ctrl+shift+n")
    type_text("Desktop test folder")
    keys("Return")
    wait_for(lambda: bool(query("SELECT id FROM folders WHERE name = 'Desktop test folder'")), "folder creation")
    keys("ctrl+n")
    type_text("A note in its own folder")
    keys("ctrl+s")
    wait_for(lambda: bool(query("SELECT n.id FROM notes n JOIN folders f ON n.folder_id = f.id WHERE n.title = 'A note in its own folder' AND f.name = 'Desktop test folder'")), "folder-scoped note")
    print("PASS: local import, folder creation, folder-scoped notes", flush=True)

    # The last typed character must survive an immediate close, not just the debounce timer.
    click(650, 345)
    type_text("Saved even when the window closes immediately.")
    close()
    assert query("SELECT body FROM notes WHERE title = 'A note in its own folder'")[0][0] == "Saved even when the window closes immediately."
    launch()
    assert query("SELECT body FROM notes WHERE title = 'A note in its own folder'")[0][0] == "Saved even when the window closes immediately."
    print("PASS: immediate-close flush and persistence across restart", flush=True)

    # Exercise the compact layout and help modal as real windows, not mocked widgets.
    run("xdotool", "windowsize", window, "840", "650")
    time.sleep(0.4)
    keys("F1")
    screenshot("compact-help.png")
    keys("Escape")
    screenshot("compact.png")
    assert process.poll() is None
    close()

    if traces:
        outbound = re.findall(r"(?:connect|sendto|sendmsg)\([^\n]*sa_family=AF_INET6?[,}]", "\n".join(trace.read_text() for trace in traces))
        assert not outbound, f"Unexpected IP network calls: {outbound}"
        print("PASS: no IPv4/IPv6 connections observed (native desktop sockets only)", flush=True)
    assert "panicked at" not in (ARTIFACTS / "application.log").read_text()
    print(f"GUI smoke test passed. Screenshots and logs: {ARTIFACTS}", flush=True)
except BaseException:
    if window and process and process.poll() is None:
        with contextlib.suppress(Exception):
            screenshot("failure.png")
    print(f"GUI test artifacts: {ARTIFACTS}", file=sys.stderr)
    raise
finally:
    for child in (process, wm):
        if child and child.poll() is None:
            child.terminate()
            with contextlib.suppress(subprocess.TimeoutExpired):
                child.wait(timeout=5)
    LOG.close()
    WM_LOG.close()
