#!/usr/bin/env python3
"""Render under an owned headless KWin/Wayland session; never use the live desktop."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from private_bus import private_bus

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    output = args.out.resolve()
    if not args.inside:
        if output.exists():
            raise SystemExit("Choose a new output directory")
        subprocess.run(["cargo", "build", "--locked", "-p", "srs-desktop", "--example", "desktop_preview"], cwd=ROOT, check=True)
        output.mkdir(parents=True)
        with tempfile.TemporaryDirectory(prefix="srs-wayland-") as directory:
            temporary = Path(directory)
            env = dict(os.environ, XDG_SESSION_TYPE="wayland", WINIT_UNIX_BACKEND="wayland",
                       LIBGL_ALWAYS_SOFTWARE="1", QT_QUICK_BACKEND="software", SRS_PRIVATE_WAYLAND="1")
            for key in ["DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_ADDRESS"]:
                env.pop(key, None)
            for key, name in [("XDG_RUNTIME_DIR", "run"), ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"), ("XDG_DATA_HOME", "data")]:
                path = temporary / name
                path.mkdir(mode=0o700)
                env[key] = str(path)
            with private_bus() as prefix:
                subprocess.run([*prefix, sys.executable, str(Path(__file__).resolve()), "--inside", "--out", str(output)], cwd=ROOT, env=env, check=True, timeout=60)
        print(f"PASS: private native Wayland render; six pages and new controls at 100%/150%. Images: {output}")
        return
    if os.environ.get("SRS_PRIVATE_WAYLAND") != "1" or "srs-wayland-" not in os.environ.get("XDG_RUNTIME_DIR", ""):
        raise SystemExit("Private runtime required")
    socket = Path(os.environ["XDG_RUNTIME_DIR"]) / "srs-test"
    with (output / "compositor.log").open("wb") as log:
        process = subprocess.Popen(["kwin_wayland", "--virtual", "--width", "2200", "--height", "1700", "--socket", socket.name,
                                    "--no-lockscreen", "--no-global-shortcuts", "--no-kactivities"], stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 15
            while not socket.exists():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("Private compositor failed; inspect compositor.log")
                time.sleep(0.05)
            for scale in ("1", "1.5"):
                env = dict(os.environ, WAYLAND_DISPLAY=socket.name, SLINT_SCALE_FACTOR=scale)
                cases = [(f"page-{page}", [f"--page={page}"]) for page in range(6)]
                cases += [("global", ["--page=3", "--global-ptt"]), ("joystick", ["--page=3", "--joystick-ptt"]), ("audio", ["--page=1", "--audio-test"])]
                for name, flags in cases:
                    subprocess.run([str(ROOT / "target/debug/examples/desktop_preview"), str(output / f"{name}-{scale}.ppm"), "--compact", *flags], cwd=ROOT, env=env, check=True, timeout=10, stdout=log, stderr=subprocess.STDOUT)
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == "__main__":
    main()
