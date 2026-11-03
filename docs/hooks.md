# Hooks

## Discovery

 - Config root candidates, in order: each entry of `CONFIGURATION_DIRECTORY` (colon-split, set by systemd
   `ConfigurationDirectory=`), then `$XDG_CONFIG_HOME/guest-agent`. Only when neither is set:
   `$HOME/.config/guest-agent`, then `/etc/guest-agent`. The packaged systemd unit uses `/etc/guest-agent`.
 - For each phase, the first candidate root that contains the phase dir (table below) is used; the other roots
   are ignored for that phase.
 - run-parts selection: filename must be non-empty and [A-Za-z0-9_-] only (no dots/extension tricks), regular file, and executable bit set (mode & 0o111). Executed in sorted order,
   sequential.

| Phase | Hooks dir |
|---|---|
| begin | snapshot.d |
| commit | snapshot-commit.d |
| completed | snapshot-completed.d |

## Execution

Each hook is executed with the following environment variables:
 - `PHASE`: The hook's phase (begin/commit/completed).
 - `VOLUME_ID`: The volume's IaaS identifier (e.g., "vol-123456").
 - `SNAPSHOT_ID`: The snapshot's IaaS identifier (e.g., "snap-789012ab").
 - `MOUNT_POINT`: Despite its name, the volume's block device in the guest (e.g., "/dev/xvdb"), not a mount
   point. To find the mounted filesystem, use `findmnt -S "$MOUNT_POINT"` (see `mount_targets_for_device` in
   `contrib/snapshot-hooks/lib.sh`).

Each hook is executed once per snapshot created in the group. If a hook interacts with an external process (e.g., a database), it must be idempotent where necessary.

### Time limit

 - begin: all hooks, for all volumes of the group, share the deadline sent by the host. If it cannot be
   parsed, the limit is 30 seconds per volume.
 - commit and completed: 30 seconds per volume and per phase.

When the deadline is reached, the running hook is killed with SIGKILL. Only the hook process is killed, not the
processes it started. The remaining hooks of that phase are killed as soon as they start, so they do not run.
Keep the hooks that must always run (e.g., a thaw) first in their phase.

## Exit Codes

Hooks should return exit code 0 for success. Non-zero exit codes will cause the snapshot operation to be considered `crash-consistent`, and the error will be logged.
A failing hook does not stop the next hooks of the same phase: they still run, and the phase is reported as failed.

## Example hooks

Examples ship in `contrib/snapshot-hooks/` and are installed by the packages under `/etc/guest-agent/` as
`<name>.disable`. Like any file whose name does not follow the run-parts selection rule, they
are skipped. To enable one, rename it to `<name>`. They source `/etc/guest-agent/lib.sh`, which must stay in
place.

| Phase dir | Hooks, in run order |
|---|---|
| snapshot.d | `40-pod-exec` (pod `begin-command`, quiesce the workload), then `50-fsfreeze` (freeze) |
| snapshot-commit.d | `90-pod-exec` (pod `commit-command`) |
| snapshot-completed.d | `50-fsfreeze` (thaw), then `90-pod-exec` (pod `completed-command`, resume the workload) |

The host snapshot is only consistent once it reaches the completed phase, so the volume must receive no writes
from the end of begin until completed. Resume the workload from `snapshot-completed.d`, sorted after the thaw,
never from `snapshot-commit.d`.
