#!/usr/bin/env python3
"""Check local Markdown file links and the dependency-free Rust workspace."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent
IGNORED = {".git", "target", ".scratch", ".venv", "__pycache__"}
LINK = re.compile(r"\[[^\]\n]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")


def check_docs():
    errors = []
    count = 0
    for directory, children, filenames in os.walk(ROOT):
        children[:] = sorted(child for child in children if child not in IGNORED)
        for filename in sorted(filenames):
            if not filename.endswith(".md"):
                continue
            source = Path(directory) / filename
            count += 1
            in_fence = False
            for number, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
                if line.lstrip().startswith(("```", "~~~")):
                    in_fence = not in_fence
                    continue
                if in_fence:
                    continue
                for match in LINK.finditer(line):
                    link = match.group(1).strip("<>")
                    parsed = urlsplit(link)
                    if parsed.scheme or parsed.netloc or not parsed.path:
                        continue
                    target = (source.parent / unquote(parsed.path)).resolve()
                    if not target.is_relative_to(ROOT) or not target.exists():
                        errors.append(f"{source.relative_to(ROOT)}:{number}: invalid local target {link}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return False
    print(f"Checked local file targets in {count} Markdown files (not anchors or remote URLs).", flush=True)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--docs-only", action="store_true", help="skip Rust build checks")
    args = parser.parse_args()
    if not check_docs():
        return 1
    if args.docs_only:
        return 0
    commands = [
        ["cargo", "fmt", "--all", "--", "--check"],
        ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"],
        ["cargo", "test", "--workspace", "--locked"],
    ]
    for command in commands:
        print("Running: " + " ".join(command), flush=True)
        try:
            subprocess.run(command, cwd=ROOT, check=True)
        except FileNotFoundError:
            print("Cargo was not found. Install Rust through rustup and reopen your terminal.", file=sys.stderr)
            return 1
        except subprocess.CalledProcessError as error:
            return error.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
