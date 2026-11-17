#!/usr/bin/env python3
"""Exercise virtual joystick and private X11/portal input, without audio."""
import json
import os
from pathlib import Path
import subprocess
import sys
from private_bus import private_bus

ROOT = Path(__file__).resolve().parent.parent


def main():
    build = subprocess.run(["cargo", "test", "--locked", "-p", "srs-input", "--lib", "--no-run", "--message-format=json"],
                           cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True)
    artifacts = [json.loads(line) for line in build.stdout.splitlines()]
    binary = next(item["executable"] for item in artifacts if item.get("reason") == "compiler-artifact"
                  and item.get("executable") and item["target"]["name"] == "srs_input")
    def run(name, prefix=(), env=None):
        subprocess.run([*prefix, binary, name, "--ignored", "--exact", "--nocapture"],
                       cwd=ROOT, env=env, check=True, timeout=25)
    # Only the explicitly created virtual joystick is read or manipulated.
    run("joystick::tests::virtual_joystick_button_and_disconnect")
    if sys.platform == "linux":
        x11 = dict(os.environ, XDG_SESSION_TYPE="x11")
        x11.pop("WAYLAND_DISPLAY", None)
        x11.pop("WAYLAND_SOCKET", None)
        run("x11::tests::private_x11_background_key_and_modifiers",
            ["xvfb-run", "-a", "-s", "-screen 0 800x600x24 -nolisten tcp"], x11)
        portal = dict(os.environ, XDG_SESSION_TYPE="wayland", WAYLAND_DISPLAY="srs-private-test", SRS_PRIVATE_INPUT_BUS="1")
        with private_bus() as prefix:
            run("portal::native_tests::private_portal_order_denial_cancellation_and_disconnect", prefix, portal)
    print("PASS: selected virtual joystick button/removal; private native input checks. No audio or shared-display input.")


if __name__ == "__main__":
    main()
