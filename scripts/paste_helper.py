#!/usr/bin/env python3
"""Proven paste helper for GNOME Wayland + keyd + ydotoold.

Port of emoji-picker.sh paste logic + inject-paste.py uinput chords.

Pipeline (what actually works for emoji):
  1. wl-copy text (and --primary)
  2. sleep ~0.35s so focus returns after a picker closes
  3. ydotool key --delay 100 --key-delay 12 super+v
     (or ctrl+shift+v when the focused app is a terminal)
  4. fallback: inject Super+V / Ctrl+V / Ctrl+Shift+V via /dev/uinput
     so keyd can grab the virtual keyboard and remap Super+V

Do NOT use `ydotool type` for emoji — it exits 0 but often injects nothing.

CLI:
  paste_helper.py --text '🍩'
  paste_helper.py --file /path/to/text
  echo 🍩 | paste_helper.py --stdin
  paste_helper.py --chord-only super+v    # assume clipboard already set
  paste_helper.py --chord-only --terminal

Environment:
  YDOTOOL_SOCKET   default /tmp/.ydotool_socket
  TIMBITS_PASTE_LOG default ~/.local/share/timbits/paste.log
  KEYD_LAST_FOCUS  default /run/user/$UID/keyd-last-focus
"""
from __future__ import annotations

import argparse
import fcntl
import logging
import os
import shutil
import struct
import subprocess
import sys
import time
from pathlib import Path
from typing import Iterable, Optional, Sequence

# ── Defaults matching emoji-picker.sh ─────────────────────────────────────

DEFAULT_YDOTOOL_SOCKET = "/tmp/.ydotool_socket"
FOCUS_RETURN_S = 0.35
YDO_KEY_DELAY_MS = "100"
YDO_KEY_STROKE_MS = "12"

# linux/input-event-codes.h
EV_SYN = 0x00
EV_KEY = 0x01
SYN_REPORT = 0
KEY_LEFTCTRL = 29
KEY_LEFTSHIFT = 42
KEY_V = 47
KEY_LEFTMETA = 125

# linux/uinput.h ioctls (verified on this kernel; same as inject-paste.py)
UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_DEV_SETUP = 0x405C5503
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
BUS_USB = 0x03
UINPUT_MAX_NAME_SIZE = 80

TERMINAL_NEEDLES = (
    "terminal",
    "kitty",
    "alacritty",
    "wezterm",
    "foot",
    "ghostty",
    "xterm",
    "konsole",
    "terminator",
    "ptyxis",
    "warp",
    "tilix",
    "guake",
    "tilda",
    "hyper",
    "rio",
    "blackbox",
    "console",
    "gnome-terminal",
    "org-gnome-terminal",
    "org-gnome-ptyxis",
    "com-mitchellh-ghostty",
)

LOG = logging.getLogger("paste_helper")


# ── Logging ───────────────────────────────────────────────────────────────

def _setup_logging(verbose: bool = False) -> None:
    level = logging.DEBUG if verbose else logging.INFO
    handlers: list[logging.Handler] = [logging.StreamHandler(sys.stderr)]
    log_path = os.environ.get(
        "TIMBITS_PASTE_LOG",
        str(Path.home() / ".local/share/timbits/paste.log"),
    )
    try:
        Path(log_path).parent.mkdir(parents=True, exist_ok=True)
        handlers.append(logging.FileHandler(log_path))
    except OSError:
        pass
    logging.basicConfig(
        level=level,
        format="%(asctime)s paste_helper: %(message)s",
        datefmt="%Y-%m-%dT%H:%M:%S",
        handlers=handlers,
        force=True,
    )


# ── Focus / terminal detection (from emoji-picker.sh) ─────────────────────

def keyd_last_focus_path() -> Path:
    env = os.environ.get("KEYD_LAST_FOCUS")
    if env:
        return Path(env)
    uid = os.getuid()
    return Path(f"/run/user/{uid}/keyd-last-focus")


def read_last_focus_class() -> str:
    """WM class from patched keyd GNOME extension (Class\\tTitle)."""
    path = keyd_last_focus_path()
    try:
        line = path.read_text(encoding="utf-8", errors="replace").splitlines()[0]
    except (OSError, IndexError):
        return ""
    return line.split("\t", 1)[0].strip()


def normalize_class(raw: str) -> str:
    out = []
    for ch in raw.lower():
        out.append(ch if ch.isalnum() else "-")
    return "".join(out)


def is_terminal_class(raw: str) -> bool:
    if not raw:
        return False
    cls = normalize_class(raw)
    return any(n in cls for n in TERMINAL_NEEDLES)


def choose_chord(
    focus_before: str = "",
    focus_after: str = "",
    force_terminal: bool = False,
    force_ctrl: bool = False,
) -> str:
    if force_terminal:
        return "ctrl+shift+v"
    if force_ctrl:
        return "ctrl+v"
    if is_terminal_class(focus_before) or is_terminal_class(focus_after):
        return "ctrl+shift+v"
    return "super+v"


# ── Clipboard ─────────────────────────────────────────────────────────────

def wl_copy(text: str, primary: bool = False) -> bool:
    cmd = ["wl-copy"]
    if primary:
        cmd.append("--primary")
    try:
        subprocess.run(
            cmd,
            input=text.encode("utf-8"),
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        return True
    except (FileNotFoundError, subprocess.CalledProcessError) as e:
        LOG.warning("wl-copy failed primary=%s: %s", primary, e)
        return False


def claim_clipboard(text: str) -> bool:
    """Mirror emoji-picker.py: clipboard + primary selection."""
    ok = wl_copy(text, primary=False)
    ok_p = wl_copy(text, primary=True)
    return ok or ok_p


# ── ydotoold ──────────────────────────────────────────────────────────────

def ydotool_socket() -> str:
    return os.environ.get("YDOTOOL_SOCKET", DEFAULT_YDOTOOL_SOCKET)


def ensure_ydotoold() -> bool:
    sock = ydotool_socket()
    if os.path.exists(sock):
        return True
    ydotoold = shutil.which("ydotoold") or (
        "/usr/bin/ydotoold" if os.path.isfile("/usr/bin/ydotoold") else None
    )
    if not ydotoold:
        return False
    try:
        subprocess.run(
            ["systemctl", "start", "ydotoold.service"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except FileNotFoundError:
        pass
    if os.path.exists(sock):
        return True
    try:
        os.remove(sock)
    except OSError:
        pass
    subprocess.Popen(
        [ydotoold],
        stdout=open("/tmp/ydotoold.spawn.log", "a"),
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    for _ in range(10):
        if os.path.exists(sock):
            return True
        time.sleep(0.05)
    return os.path.exists(sock)


def ydotool_bin() -> Optional[str]:
    if os.path.isfile("/usr/bin/ydotool"):
        return "/usr/bin/ydotool"
    return shutil.which("ydotool")


def ydotool_key(chord: str) -> bool:
    """emoji-picker.sh: ydotool key --delay 100 --key-delay 12 <chord>."""
    bin_ = ydotool_bin()
    if not bin_:
        LOG.warning("ydotool not found")
        return False
    ensure_ydotoold()
    env = os.environ.copy()
    env["YDOTOOL_SOCKET"] = ydotool_socket()
    try:
        r = subprocess.run(
            [
                bin_,
                "key",
                "--delay",
                YDO_KEY_DELAY_MS,
                "--key-delay",
                YDO_KEY_STROKE_MS,
                chord,
            ],
            env=env,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        if r.returncode == 0:
            LOG.info("ydotool ok (%s)", chord)
            return True
        LOG.warning("ydotool failed (%s): %s", chord, (r.stderr or "").strip())
        return False
    except OSError as e:
        LOG.warning("ydotool spawn failed: %s", e)
        return False


# ── uinput inject (from inject-paste.py) ──────────────────────────────────

def _pack_input_event(etype: int, code: int, value: int) -> bytes:
    now = time.time()
    sec = int(now)
    usec = int((now - sec) * 1_000_000)
    return struct.pack("llHHi", sec, usec, etype, code, value)


def _pack_uinput_setup(name: str) -> bytes:
    name_bytes = name.encode("utf-8")[: UINPUT_MAX_NAME_SIZE - 1]
    name_padded = name_bytes + b"\0" * (UINPUT_MAX_NAME_SIZE - len(name_bytes))
    return struct.pack("HHHH", BUS_USB, 0x0001, 0x0001, 1) + name_padded + struct.pack("I", 0)


def inject_uinput_chord(keys_down: Sequence[int], device_name: str = "timbits-paste") -> None:
    """Press keys_down in order, release reverse. keyd may grab this device."""
    if not os.path.exists("/dev/uinput"):
        raise RuntimeError("/dev/uinput not found")

    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    try:
        fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
        fcntl.ioctl(fd, UI_SET_EVBIT, EV_SYN)
        for key in set(list(keys_down) + [KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V, KEY_LEFTMETA]):
            fcntl.ioctl(fd, UI_SET_KEYBIT, key)

        fcntl.ioctl(fd, UI_DEV_SETUP, _pack_uinput_setup(device_name))
        fcntl.ioctl(fd, UI_DEV_CREATE)

        # Give udev/keyd a beat to notice and grab (emoji-picker uses 0.15s).
        time.sleep(0.15)

        def emit(etype: int, code: int, value: int) -> None:
            os.write(fd, _pack_input_event(etype, code, value))

        def sync() -> None:
            emit(EV_SYN, SYN_REPORT, 0)

        for key in keys_down:
            emit(EV_KEY, key, 1)
        sync()
        time.sleep(0.02)
        for key in reversed(list(keys_down)):
            emit(EV_KEY, key, 0)
        sync()
        time.sleep(0.05)
        fcntl.ioctl(fd, UI_DEV_DESTROY)
    finally:
        os.close(fd)


def chord_to_keys(chord: str) -> list[int]:
    c = chord.lower().replace(" ", "")
    if c in ("ctrl+shift+v", "control+shift+v"):
        return [KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V]
    if c in ("ctrl+v", "control+v"):
        return [KEY_LEFTCTRL, KEY_V]
    if c in ("super+v", "meta+v", "cmd+v"):
        return [KEY_LEFTMETA, KEY_V]
    raise ValueError(f"unknown chord: {chord}")


def inject_chord_name(chord: str) -> bool:
    try:
        inject_uinput_chord(chord_to_keys(chord))
        LOG.info("uinput inject ok (%s)", chord)
        return True
    except Exception as e:
        LOG.warning("uinput inject failed (%s): %s", chord, e)
        return False


# ── Full paste pipeline ───────────────────────────────────────────────────

def paste_chord(chord: str) -> bool:
    """Try ydotool key, then uinput — same order as emoji-picker.sh."""
    LOG.info("paste chord=%s socket=%s", chord, ydotool_socket())
    if ydotool_key(chord):
        return True
    if inject_chord_name(chord):
        return True
    return False


def paste_text(
    text: str,
    *,
    focus_before: str = "",
    skip_focus_wait: bool = False,
    force_terminal: bool = False,
    force_ctrl: bool = False,
) -> bool:
    """
    Full paste: clipboard → wait → Super+V / Ctrl+Shift+V.

    Call after the picker has closed. If you snapshotted keyd-last-focus before
    opening the picker, pass it as focus_before for terminal detection.
    """
    if not text:
        LOG.warning("empty text; nothing to paste")
        return False

    if not claim_clipboard(text):
        LOG.error("clipboard claim failed")
        return False
    LOG.info("clipboard claimed (%d bytes)", len(text.encode("utf-8")))

    if not skip_focus_wait:
        time.sleep(FOCUS_RETURN_S)

    focus_after = read_last_focus_class()
    if not focus_before:
        focus_before = focus_after
    LOG.info("focus_before=%r focus_after=%r", focus_before or "<empty>", focus_after or "<empty>")

    chord = choose_chord(
        focus_before=focus_before,
        focus_after=focus_after,
        force_terminal=force_terminal,
        force_ctrl=force_ctrl,
    )
    ok = paste_chord(chord)
    if not ok:
        LOG.error("all paste methods failed; text is on clipboard — press Super+V")
    return ok


# ── CLI ───────────────────────────────────────────────────────────────────

def main(argv: Optional[Sequence[str]] = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    src = p.add_mutually_exclusive_group()
    src.add_argument("--text", help="Text/emoji to paste")
    src.add_argument("--file", type=Path, help="Read text from file")
    src.add_argument("--stdin", action="store_true", help="Read text from stdin")
    src.add_argument(
        "--chord-only",
        action="store_true",
        help="Only inject paste chord (clipboard already set)",
    )
    p.add_argument(
        "--focus-before",
        default="",
        help="WM class snapped before the picker opened (terminal detection)",
    )
    p.add_argument("--terminal", action="store_true", help="Force Ctrl+Shift+V")
    p.add_argument("--ctrl", action="store_true", help="Force Ctrl+V")
    p.add_argument(
        "--no-wait",
        action="store_true",
        help="Skip focus-return sleep (caller already waited / restored focus)",
    )
    p.add_argument("-v", "--verbose", action="store_true")
    args = p.parse_args(argv)
    _setup_logging(args.verbose)

    if args.chord_only:
        chord = choose_chord(
            focus_before=args.focus_before or read_last_focus_class(),
            force_terminal=args.terminal,
            force_ctrl=args.ctrl,
        )
        return 0 if paste_chord(chord) else 1

    text = ""
    if args.text is not None:
        text = args.text
    elif args.file is not None:
        text = args.file.read_text(encoding="utf-8")
    elif args.stdin:
        text = sys.stdin.read()
    else:
        p.error("provide --text, --file, --stdin, or --chord-only")

    # Strip a single trailing newline from CLI/file convenience, keep emoji intact.
    if text.endswith("\n") and text.count("\n") == 1:
        text = text[:-1]

    ok = paste_text(
        text,
        focus_before=args.focus_before,
        skip_focus_wait=args.no_wait,
        force_terminal=args.terminal,
        force_ctrl=args.ctrl,
    )
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
