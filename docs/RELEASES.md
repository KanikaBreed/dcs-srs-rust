# Release and update policy

There are no application releases or updater yet. Repository initialization is not a release. Publish releases or submit to an app store only when requested by a maintainer.

## Future release checklist

1. Select an explicit version and reviewed source commit; distinguish app version from SRS compatibility version.
2. Verify relevant CI for that commit and actual device/interoperability evidence.
3. Build platform artifacts from that source with the committed dependency lockfile.
4. Record supported OS/device combinations, tested SRS versions, known limitations, and upgrade behavior.
5. Verify redistribution notices, source availability, native dependencies, and signing ownership.
6. Check artifact hashes, executable version reporting, installation, rollback, and configuration preservation.
7. Publish accurate notes and source references; update README and tasks.

Use semantic versions for application releases. Keep prereleases clearly labeled. Platform packages, update signatures, a release workflow, a store pipeline, and a task-archive script are future work, not assumed infrastructure.

## Platform distribution

Desktop packaging must preserve user configuration and avoid overwriting an upstream SRS installation. Android distribution needs proper signing and runtime-permission behavior. iOS distribution requires a verified licensing and signing route. Signing keys, store credentials, and APNs credentials stay in appropriate private infrastructure, never in Git.

An eventual updater must authenticate downloaded artifacts and handle interrupted updates/rollback. Design it as a separate reviewed feature. There is no automatic update behavior in the scaffold.
