#!/usr/bin/env python3
"""Run the real Linux file-dialog adapter with a private simulated portal."""
import json
import os
from pathlib import Path
import subprocess
from private_bus import private_bus

ROOT = Path(__file__).resolve().parent.parent


def main():
    build = subprocess.run(["cargo", "test", "--locked", "-p", "srs-desktop", "--lib", "--no-run", "--message-format=json"], cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True)
    artifacts = [json.loads(line) for line in build.stdout.splitlines()]
    binary = next(item["executable"] for item in artifacts if item.get("reason") == "compiler-artifact" and item.get("executable") and item["target"]["name"] == "srs_desktop")
    env = dict(os.environ, SRS_PRIVATE_CHOOSER="1")
    # No fallback helper can reach the user's window system.
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET"]:
        env.pop(key, None)
    with private_bus() as prefix:
        subprocess.run([*prefix, binary, "profiles::dialog_tests::private_native_chooser_cancel_export_import_and_conflicts", "--ignored", "--exact", "--nocapture"], cwd=ROOT, env=env, check=True, timeout=25)
    print("PASS: real native portal request/response, cancellation, export/import, existing destination, conflict and malformed input. No shared desktop.")


if __name__ == "__main__":
    main()
