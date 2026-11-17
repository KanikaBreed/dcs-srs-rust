#!/usr/bin/env python3
"""Generate or verify fixtures with pinned, unchanged upstream C# wire code."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
REVISION = "d01f540ce5e363e9ac841775c8f7e2253138cd81"
FIXTURES = ROOT / "crates/srs-protocol/tests/fixtures"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--write", action="store_true", help="replace committed fixture files")
    parser.add_argument("--verify-dir", type=Path, help="also validate Rust round-trip output with C#")
    args = parser.parse_args()
    reference = args.reference.resolve()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=reference, text=True).strip()
    if revision != REVISION:
        raise SystemExit(f"Expected upstream {REVISION}, got {revision}")
    dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=reference, text=True)
    if dirty:
        raise SystemExit("Reference has tracked changes; use a clean checkout")
    project = ROOT / "tools/reference-fixtures/ReferenceFixtures.csproj"
    command = ["dotnet", "run", "--project", str(project), f"-p:SrsReferenceRoot={reference}", "--"]
    env = dict(os.environ, DOTNET_CLI_TELEMETRY_OPTOUT="1", DOTNET_NOLOGO="1")
    scratch = ROOT / ".scratch"
    scratch.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="srs-fixtures-", dir=scratch) as temporary:
        output = Path(temporary)
        subprocess.run(command + ["--write", str(output)], cwd=ROOT, env=env, check=True)
        # Also compare against the literal byte arrays in upstream's unit tests;
        # our generator is not the sole source of expected UDP bytes.
        tests = (reference / "DCS-SR-CommonTests/Network/UDPVoicePacketTests.cs").read_text(encoding="utf-8-sig")
        arrays = re.findall(r"new byte\[(?:79|99)\]\s*\{(.*?)\};", tests, re.S)
        if len(arrays) != 4:
            raise SystemExit("Upstream golden vector structure changed")
        for name, array in zip(["single-voice.bin", "single-relayed.bin", "multi-voice.bin", "multi-voice.bin"], arrays):
            body = re.sub(r"//[^\n]*", "", array)
            expected = bytes(int(x.strip()) for x in body.split(",") if x.strip())
            if (output / name).read_bytes() != expected:
                raise SystemExit(f"Published upstream test vector differs: {name}")
        FIXTURES.mkdir(parents=True, exist_ok=True)
        for source in output.iterdir():
            target = FIXTURES / source.name
            if args.write:
                target.write_bytes(source.read_bytes())
            elif not target.exists() or target.read_bytes() != source.read_bytes():
                raise SystemExit(f"Committed fixture differs: {source.name}")
    if args.verify_dir:
        subprocess.run(command + ["--verify", str(args.verify_dir.resolve())], cwd=ROOT, env=env, check=True)
    print("Verified pinned upstream output and all four published UDP vectors.")


if __name__ == "__main__":
    main()
