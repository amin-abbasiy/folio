#!/usr/bin/env python3
"""Clipboard and drag-selection regression tests; run on a private xvfb-run display."""

import contextlib
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "target/debug/folio").resolve()
ARTIFACTS = Path(tempfile.mkdtemp(prefix="folio-selection-"))
DATA = ARTIFACTS / "notebook"
ENV = dict(os.environ, LIBGL_ALWAYS_SOFTWARE="1", XDG_RUNTIME_DIR=str(ARTIFACTS / "runtime"))
Path(ENV["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
process = wm = None
window = None
log = (ARTIFACTS / "application.log").open("w")


def run(*args, check=True):
    return subprocess.run(args, env=ENV, check=check, capture_output=True, text=True, timeout=5)


def keys(sequence):
    run("xdotool", "key", "--clearmodifiers", sequence)
    time.sleep(0.2)


def move(x, y):
    run("xdotool", "mousemove", "--window", window, str(x), str(y))


def click(x, y):
    move(x, y)
    run("xdotool", "click", "1")
    time.sleep(0.2)


def clipboard():
    return run("xclip", "-selection", "clipboard", "-out").stdout


def screenshot(name):
    run("import", "-display", ENV["DISPLAY"], "-window", window, str(ARTIFACTS / name))


def drag(start, end, seconds, until=None):
    move(*start)
    run("xdotool", "mousedown", "1")
    time.sleep(0.15)
    move(*end)
    # Do not move the pointer again: scrolling must continue while held still.
    if until is None:
        time.sleep(seconds)
    else:
        # Renderers differ in speed. Wait for the actual copied selection rather
        # than assuming a fixed number of pixels/paragraphs per second.
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            time.sleep(0.4)
            # --clearmodifiers also releases held mouse buttons in xdotool,
            # which would end the very selection gesture we are testing.
            run("xdotool", "key", "ctrl+c")
            time.sleep(0.2)
            if until(clipboard()):
                break
        else:
            raise AssertionError("Selection did not reach the requested text while holding the stationary pointer")
    run("xdotool", "mouseup", "1")
    time.sleep(0.2)
    keys("ctrl+c")
    return clipboard()


def scroll_top():
    move(900, 500)
    run("xdotool", "click", "--repeat", "50", "--delay", "10", "4")
    time.sleep(0.6)


def wait_for(predicate, message):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if predicate():
            return
        if process.poll() is not None:
            raise AssertionError(f"App exited: {process.returncode}")
        time.sleep(0.08)
    raise AssertionError(f"Timed out: {message}")


def query(sql):
    with contextlib.closing(sqlite3.connect(DATA / "notes.sqlite3")) as connection:
        return connection.execute(sql).fetchall()


try:
    assert ENV.get("DISPLAY"), "Use xvfb-run, not a host desktop."
    wm = subprocess.Popen(["openbox", "--sm-disable"], env=ENV, stdout=log, stderr=log)
    time.sleep(0.4)
    process = subprocess.Popen([str(BINARY), "--data-dir", str(DATA)], env=ENV, stdout=log, stderr=log)

    def find_window():
        global window
        result = run("xdotool", "search", "--all", "--onlyvisible", "--pid", str(process.pid), "--name", "^Folio$", check=False)
        if result.returncode == 0 and result.stdout.strip():
            window = result.stdout.strip().splitlines()[-1]
            return True
        return False

    wait_for(find_window, "Folio's own window")
    run("xdotool", "windowactivate", "--sync", window)
    time.sleep(0.5)
    title = "Selection regression"
    body = "\n\n".join(f"Line {number:03}: café 🌿 — keep selecting this text beyond the visible page." for number in range(1, 61))
    imported = ARTIFACTS / "long-note.md"
    imported.write_text(f"# {title}\n\n{body}")
    keys("ctrl+o")
    keys("ctrl+a")
    run("xdotool", "type", "--clearmodifiers", "--delay", "5", "--", str(imported))
    time.sleep(0.2)
    keys("Return")
    wait_for(lambda: query("SELECT count(*) FROM notes WHERE title = 'Selection regression'") == [(1,)], "long note import")
    screenshot("long-note.png")

    copied = drag((538, 280), (1000, 766), 1.8)
    screenshot("read-selection-partial.png")
    # The initial viewport contains only 12 paragraphs. Test actual progress,
    # without assuming a particular renderer's frame rate or edge-scroll speed.
    assert "Line 001" in copied and "Line 014" in copied, repr(copied)
    assert "Line 060" not in copied, "Selection jumped to the end instead of following bounded scrolling"
    print("PASS: reading selection scrolls with a stationary pointer and retains its offscreen anchor", flush=True)

    scroll_top()
    copied = drag((538, 280), (1000, 780), 1.8)
    assert "Line 014" in copied and "Line 060" not in copied, "Dragging outside the pane jumped the selection to the document end"
    print("PASS: dragging past the edge extends selection gradually, without jumping to the end", flush=True)

    scroll_top()
    copied = drag((538, 280), (1000, 780), 30.0, until=lambda text: text == body)
    screenshot("read-selection-down.png")
    assert copied == body, "Selection did not preserve every paragraph and Unicode character: " + repr(copied)
    print("PASS: selecting downward beyond the viewport copies offscreen paragraphs", flush=True)

    copied = drag((950, 710), (538, 199), 30.0, until=lambda text: "Line 001" in text and "Line 050" in text)
    screenshot("read-selection-up.png")
    assert "Line 001" in copied and "Line 050" in copied, repr(copied)
    print("PASS: selection auto-scrolls upward without losing the lower anchor", flush=True)

    click(1070, 103)
    assert clipboard() == f"{title}\n\n{body}", "Copy note did not copy the complete title and body"
    keys("ctrl+shift+c")
    assert clipboard() == f"{title}\n\n{body}", "Whole-note shortcut copied only the selected text"
    print("PASS: Copy note button and Ctrl+Shift+C copy the entire note", flush=True)

    keys("ctrl+Return")
    scroll_top()
    screenshot("write-before-selection.png")
    copied = drag((538, 324), (1000, 766), 1.8)
    screenshot("write-selection-partial.png")
    assert "Line 001" in copied and "Line 014" in copied, repr(copied)
    print("PASS: writing selection auto-scrolls beyond the visible editor", flush=True)

    # Copy must use the live draft, without waiting for the autosave debounce.
    click(800, 500)
    keys("ctrl+End")
    run("xdotool", "type", "--clearmodifiers", "--delay", "4", "--", " UNSAVED-COPY-CHECK")
    time.sleep(0.2)
    keys("ctrl+shift+c")
    wait_for(lambda: clipboard().endswith(" UNSAVED-COPY-CHECK"), "latest draft on the system clipboard")
    assert clipboard().startswith(title + "\n\n")
    print("PASS: whole-note copy includes the latest draft", flush=True)

    keys("alt+F4")
    process.wait(timeout=10)
    assert process.returncode == 0
    print(f"Selection regression tests passed. Artifacts: {ARTIFACTS}", flush=True)
except BaseException:
    run("xdotool", "mouseup", "1", check=False)
    if window and process and process.poll() is None:
        with contextlib.suppress(Exception):
            (ARTIFACTS / "clipboard-on-failure.txt").write_text(clipboard())
            screenshot("failure.png")
    print(f"Selection test artifacts: {ARTIFACTS}", file=sys.stderr)
    raise
finally:
    for child in (process, wm):
        if child and child.poll() is None:
            child.terminate()
            with contextlib.suppress(subprocess.TimeoutExpired):
                child.wait(timeout=5)
    log.close()
