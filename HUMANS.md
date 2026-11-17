# Human contributor guide

The initial product is an SRS-compatible TOC radio console. Read [scope](docs/SCOPE.md) and [tasks](TASKS.md) for the current boundary. Agents follow [AGENTS.md](AGENTS.md).

## Getting started

Clone the repository and install Rust through rustup, your host's native build tools, and Python 3.11+. Run `python3 scripts/check.py`, or `py -3 scripts/check.py` on Windows. The initial workspace has no native audio dependencies and no app to launch.

For implementation work, fetch the latest branch and create an isolated task worktree:

```sh
git fetch origin
git worktree add ../work/dcs-srs-rust/voice-packets -b feat/voice-packets origin/main
```

Run builds in that worktree. Put temporary logs in `.scratch/`. No custom task launcher, build-slot system, wiki generator, or cleanup daemon is installed.

## Working with an agent

1. State the behavior you want and any test server/device authorization.
2. The agent examines SRS behavior, scopes the change, and updates TASKS.md.
3. It implements and runs suitable automated checks.
4. For audible, hardware, mobile, or cockpit behavior, it supplies a concrete build and short validation steps.
5. Record the device/OS, source revision, observed result, and remaining limits. Do not post passwords or voice recordings from other users.
6. Review a PR with its checks and evidence. Drafts may exist before device validation.
7. Merge within the project's configured permissions. Releases and store submissions are separate requested actions.

A successful build does not establish background operation, acceptable latency, or radio correctness. Human listening and actual device tests remain part of those acceptance criteria.

## Repository status

- README describes what exists now.
- ARCHITECTURE distinguishes implemented files from planned components.
- TASKS tracks work and validation gaps.
- docs/PROTOCOL records the SRS baseline and compatibility decisions.
- docs/TESTING defines evidence and platform acceptance.

CI runs Windows, Linux, and macOS host checks. It does not build an Android/iOS app yet. The initial scaffold passed all three hosts; the run is linked in TASKS.md.

`main` requires a pull request and all three host CI checks on an up-to-date branch. The rules apply to administrators, and force pushes and branch deletion are disabled. Squash merging is enabled. No second-person approval is required by GitHub at bootstrap; self-review does not count as independent review. Recheck the live GitHub settings and checks when merging, because repository configuration can change.

## Public contributions

Use English in code and GitHub artifacts. Keep reports focused and redact server addresses, account information, passwords, device serials, and personal paths. [Bug reporting](docs/BUG_REPORTING.md) explains the useful diagnostics. Use [security reporting](SECURITY.md) for sensitive findings.

Use the existing authenticated GitHub account. Never paste access tokens into issues, prompts, configuration examples, or repository files. Contributors can use their GitHub noreply address for public commits.
