#!/usr/bin/env python3
"""Exercise CPAL/ALSA against a process-local null PCM; no physical devices."""
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def main():
    if sys.platform != "linux":
        raise SystemExit("The null-PCM native probe requires Linux/ALSA")
    subprocess.run(["cargo", "build", "--locked", "-p", "srs-device", "--bin", "audio_probe"], cwd=ROOT, check=True)
    scratch = ROOT / ".scratch"
    scratch.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="srs-null-audio-", dir=scratch) as temporary:
        config = Path(temporary) / "alsa.conf"
        # This is a complete replacement config, with no includes, plugins that
        # reach hardware, or user's system/default route. Only this child sees it.
        config.write_text("pcm.!default { type null }\n", encoding="utf-8")
        env = dict(os.environ, ALSA_CONFIG_PATH=str(config))
        executable = ROOT / "target/debug/audio_probe"
        for mode, flag, marker in [("tone", "--output", "Playback"), ("capture", "--input", "Capture")]:
            result = subprocess.run([str(executable), mode, flag, "default", "--seconds", "1"],
                                    cwd=ROOT, env=env, timeout=15,
                                    capture_output=True, text=True)
            print(result.stdout, end="")
            if result.stderr:
                print(result.stderr, file=sys.stderr, end="")
            result.check_returncode()
            match = re.search(rf"{marker} frames=(\d+).*stopped=false", result.stdout)
            if match is None or int(match[1]) == 0:
                raise SystemExit(f"{mode}: native null stream did not process frames")
    print("PASS: native ALSA null input/output open, callbacks, codec frames, and clean process exit. "
          "Null PCM is unpaced; drop/underrun counts are not hardware timing measurements.")


if __name__ == "__main__":
    main()
