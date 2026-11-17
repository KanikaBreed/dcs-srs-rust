# Aurora documentation adaptation

Reviewed on 2026-11-16. Source: [Aurora Viewer revision 656e520c961ff9c5f04df693fcaa9371dabd48b2](https://github.com/Aurora-Viewer/Aurora-Viewer/tree/656e520c961ff9c5f04df693fcaa9371dabd48b2).

The review inventoried all 39 tracked Markdown files, read the agent instructions, and examined applicable contributor, architecture, testing, platform, branding, reporting, translation, and release guidance. Large viewer-specific guides were classified by subject and relevant sections; they were not ported line by line. No tracked `.mo` files were present; the request was interpreted as Markdown guidance. Untracked local settings and private notes are not included.

## Adaptation decisions

- Keep shared-worktree discipline, upstream behavior references, explicit task status, honest validation, consistent UI, and attributed code/assets.
- Replace grid restrictions with offline/loopback defaults and explicitly authorized shared-server voice tests.
- Replace GPU/frame-rate rules with bounded real-time audio, latency, dropout, and mobile power requirements.
- Keep humans involved in listening/device acceptance, while permitting draft PRs and routine documentation/tooling work without another approval gate.
- Use task paths scoped to this project; do not sweep a shared work directory or modify another agent's changes.
- Keep the user's current authorization authoritative; do not automatically publish comments, merge unrelated PRs, or manage another project's backlog.
- Do not carry over Aurora's branding, secrets, production services, organization board, special labels, Windows-only scripts, or claims of existing tooling.
- Separate current scaffold checks from planned functional, native, and mobile tests.

## File-by-file disposition

| Aurora source | Decision in this project |
|---|---|
| `.github/pull_request_template.md` | Adapt to radio behavior, upstream provenance, audio/device evidence, and pending validation. |
| `.github/release-notes-header.md` | Replace release assumptions with docs/RELEASES.md; no release workflow yet. |
| `AGENTS.md` | Rewrite as AGENTS.md: SRS compatibility, audio constraints, scoped authorization, worktrees, evidence, and review. |
| `ARCHITECTURE.md` | Replace viewer architecture with the Rust radio core, transport, audio, UI, and native adapter plan. |
| `CLAUDE.md` | Keep a single @AGENTS.md pointer; no duplicated policy. |
| `HUMANS.md` | Rewrite as HUMANS.md with working bootstrap commands and device/listening validation. |
| `NOTICE.md` | Retain workflow attribution in NOTICE.md; do not claim unused third-party assets are bundled. |
| `README.md` | Rewrite for a standalone TOC client; state pre-implementation status. |
| `TASKS.md` | Start a new TASKS.md for protocol/TOC/mobile milestones; import no viewer tasks. |
| `TRADEMARKS.md` | Carry independent branding/attribution principles into NOTICE.md and docs/BRANDING.md; copy no marks. |
| `assets/hrtf/README.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `assets/models/README.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `assets/stars/LICENSE-HYG.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `assets/stars/README.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `crates/aurora-assets/native/vhacd/README.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `crates/aurora-viewer/assets/animations/locomotion/README.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `crates/aurora-viewer/assets/animations/swimming/README.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `crates/aurora-viewer/assets/dictionaries/README.md` | Exclude viewer assets/dependency material; retain the general provenance discipline in NOTICE.md. |
| `docs/ANIMATION_CREATION.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/BLENDER_LIVE.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/BRANDING.md` | Rewrite as docs/BRANDING.md for legible TOC controls and accessible TX/RX state. |
| `docs/BUG_REPORTING.md` | Adapt to redacted connection/audio/PTT diagnostics in docs/BUG_REPORTING.md. |
| `docs/DEV_TOOLBOX.md` | Adapt diagnostics principles into docs/BUG_REPORTING.md; no invented toolbox commands. |
| `docs/HAND_CONTACTS.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/IK_EDIT.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/IMPORTS.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/LINUX.md` | Replace viewer installation claims with Linux audio/PTT/package work in docs/PLATFORMS.md. |
| `docs/MACOS.md` | Use Apple build/signing concepts for iOS in docs/PLATFORMS.md; no macOS client promise. |
| `docs/POSER.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/RLVA.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/TASKS_ARCHIVE.md` | Do not import historical viewer work; archive only this project when needed. |
| `docs/TESTING.md` | Rewrite as docs/TESTING.md for packet fixtures, synthetic audio, stock SRS interoperability, and native lifecycle. |
| `docs/TRANSLATING.md` | Adapt catalogue discipline into docs/TRANSLATING.md; English first, runtime/catalogues pending. |
| `docs/UPDATES.md` | Adapt packaging, identity, rollback, and user-requested release principles into docs/RELEASES.md. |
| `docs/VR.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `docs/VR_WINDOWS.md` | Exclude viewer-specific functionality; no direct counterpart in the standalone radio scope. |
| `news/en.md` | Do not import Aurora release history or translations; write project-specific notes when releases exist. |
| `news/fr.md` | Do not import Aurora release history or translations; write project-specific notes when releases exist. |
| `news/ja.md` | Do not import Aurora release history or translations; write project-specific notes when releases exist. |

Attribution and licensing are recorded in [NOTICE.md](../NOTICE.md). The replacement guides describe this project's actual state and planned work, rather than mechanically renaming Aurora paths.
