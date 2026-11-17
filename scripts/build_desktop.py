#!/usr/bin/env python3
"""Prepare a local validation build, source/toolchain identity and checksums.

This does not install, sign, upload or publish anything. It is not a distribution
package: native dependencies and complete dependency/source notices still need
the release gate in docs/RELEASES.md. No microphone or network session is opened.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", action="store_true", help="Use the optimized Cargo release profile")
    parser.add_argument("--out", type=Path, help="New output directory (defaults to .scratch/desktop-build)")
    args = parser.parse_args()
    output = args.out or ROOT / ".scratch" / "desktop-build"
    if output.exists():
        raise SystemExit("Choose a new output directory; existing validation builds are preserved.")
    revision = command("git", "rev-parse", "HEAD")
    state = command("git", "status", "--porcelain", "--untracked-files=normal")
    lock_hash = digest(ROOT / "Cargo.lock")
    build = ["cargo", "build", "--locked", "-p", "srs-desktop", "--bin", "srs-desktop", "--message-format=json-render-diagnostics"]
    if args.release:
        build.append("--release")
    executable = None
    with subprocess.Popen(build, cwd=ROOT, stdout=subprocess.PIPE, text=True) as process:
        for line in process.stdout:
            event = json.loads(line)
            if event.get("reason") == "compiler-message":
                print(event["message"].get("rendered", ""), end="", file=sys.stderr)
            if (event.get("reason") == "compiler-artifact" and event.get("executable")
                    and event["target"]["name"] == "srs-desktop"
                    and "bin" in event["target"]["kind"]):
                executable = Path(event["executable"])
        if process.wait():
            raise SystemExit("Desktop build failed; no validation directory was created.")
    if executable is None:
        raise SystemExit("Cargo did not report a desktop executable.")
    if revision != command("git", "rev-parse", "HEAD") or state != command("git", "status", "--porcelain", "--untracked-files=normal") or lock_hash != digest(ROOT / "Cargo.lock"):
        raise SystemExit("Source state changed while building; rerun after edits are complete.")
    output.mkdir(parents=True, exist_ok=False)
    shutil.copy2(executable, output / executable.name)
    for source in ["LICENSE", "NOTICE.md", "Cargo.lock"]:
        shutil.copy2(ROOT / source, output / Path(source).name)
    shutil.copytree(ROOT / "docs", output / "docs")
    (output / "README.txt").write_text(
        f"Local validation build. Launch {executable.name} from this directory.\n"
        "See docs/DESKTOP_VALIDATION.md and manifest.json before hardware testing.\n"
        "This is not a signed installer or a complete redistribution package.\n", encoding="utf-8")
    files = {p.relative_to(output).as_posix(): digest(p) for p in sorted(output.rglob("*")) if p.is_file()}
    manifest = {
        "format": 1,
        "purpose": "Local development validation; not a signed installer or redistribution package",
        "source_revision": revision,
        "source_dirty": bool(state),
        "source_url": f"https://github.com/KanikaBreed/dcs-srs-rust/tree/{revision}",
        "profile": "release" if args.release else "dev",
        "rustc": command("rustc", "--version", "--verbose"),
        "cargo": command("cargo", "--version"),
        "files_sha256": files,
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Prepared {output}; dirty source: {bool(state)}. No app was launched.")


if __name__ == "__main__":
    main()
