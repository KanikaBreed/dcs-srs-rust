#!/usr/bin/env python3
"""Exchange generated Opus frames with unchanged upstream C# codec wrappers."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
REVISION = "d01f540ce5e363e9ac841775c8f7e2253138cd81"

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, type=Path)
    args = parser.parse_args()
    reference = args.reference.resolve()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=reference, text=True).strip()
    dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=reference, text=True)
    if head != REVISION or dirty:
        raise SystemExit("Codec reference must be the clean pinned upstream revision")
    scratch = ROOT / ".scratch"
    scratch.mkdir(exist_ok=True)
    env = dict(os.environ, DOTNET_CLI_TELEMETRY_OPTOUT="1", DOTNET_NOLOGO="1")
    with tempfile.TemporaryDirectory(prefix="codec-interop-", dir=scratch) as temporary:
        rust = ["cargo", "run", "--locked", "-p", "srs-audio", "--example", "codec_probe", "--"]
        subprocess.run(rust + ["write", temporary], cwd=ROOT, check=True)
        subprocess.run(["dotnet", "run", "--project", str(ROOT / "tools/voice-reference/VoiceReference.csproj"),
                        f"-p:SrsReferenceRoot={reference}", "--", temporary], cwd=ROOT, env=env, check=True)
        subprocess.run(rust + ["verify", temporary], cwd=ROOT, check=True)

if __name__ == "__main__":
    main()
