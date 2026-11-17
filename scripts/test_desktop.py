#!/usr/bin/env python3
"""GUI/session smoke check inside test_stock_server's private network namespace."""
import ctypes
import ctypes.util
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


def inside_private_display(test_binary):
    # Xvfb has no window manager to grant activation requests. Only focus our
    # synthetic test window on xvfb-run's private, authenticated display.
    if "xvfb-run." not in os.environ.get("XAUTHORITY", ""):
        raise SystemExit("Refusing to focus a window outside xvfb-run")
    process = subprocess.Popen([
        str(test_binary), "app::tests::desktop_session_smoke", "--ignored", "--exact", "--nocapture",
    ], cwd=ROOT)
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and process.poll() is None:
            tree = subprocess.check_output(["xwininfo", "-root", "-tree"], text=True)
            match = re.search(r'(0x[0-9a-fA-F]+) "DCS SRS Rust .*UI PREVIEW', tree)
            if match and "Map State: IsViewable" not in subprocess.check_output(["xwininfo", "-id", match[1]], text=True):
                time.sleep(0.05)
                continue
            if match:
                x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
                x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
                x11.XOpenDisplay.restype = ctypes.c_void_p
                x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
                x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
                display = x11.XOpenDisplay(None)
                if not display:
                    raise RuntimeError("Cannot open private Xvfb display")
                try:
                    x11.XSetInputFocus(display, int(match[1], 16), 2, 0)
                finally:
                    x11.XCloseDisplay(display)  # flushes this connection
                break
            time.sleep(0.05)
        else:
            raise RuntimeError("Synthetic desktop window did not appear")
        if process.wait(timeout=30):
            raise RuntimeError("Desktop session test failed")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()


def verify_desktop(address, test_binary):
    with tempfile.TemporaryDirectory(prefix="desktop-null-", dir=ROOT / ".scratch") as temporary:
        config = Path(temporary) / "alsa.conf"
        config.write_text("pcm.!default { type null }\n", encoding="utf-8")
        env = dict(os.environ, ALSA_CONFIG_PATH=str(config), SRS_DESKTOP_TEST_SERVER=address,
                   WINIT_UNIX_BACKEND="x11", XDG_SESSION_TYPE="x11")
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("WAYLAND_SOCKET", None)
        subprocess.run([
            "xvfb-run", "-a", "-s", "-screen 0 1400x1000x24 -nolisten tcp",
            sys.executable, str(Path(__file__).resolve()), "--inside-private-display", str(test_binary),
        ], cwd=ROOT, env=env, check=True, timeout=45)
        print("PASS: desktop window/controller authenticated with stock SRS; disabled-radio start, "
              "generated stock-server RX/indicator, native null capture/TX, UI-stall release, stale-input rejection, pointer cancel, "
              "tune/guard/key/off, hold cap, saved server/preset reload, offline preparation, "
              "hostname reconnect, forced TCP loss/recovery without held TX, menus, server policy, device refresh, PTT rebinding/persistence, live preset release, receive levels/routing, per-radio RX, focused Space/Tab release, offline diagnostic/session exclusion and disconnect/join. No hardware.", flush=True)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--inside-private-display":
        inside_private_display(Path(sys.argv[2]))
    else:
        raise SystemExit("Run python3 scripts/test_stock_server.py; this helper requires its private namespace.")
