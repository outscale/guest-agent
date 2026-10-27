# Hooks

## Discovery

 - Config root, first existing wins: CONFIGURATION_DIRECTORY (colon-split, systemd ConfigurationDirectory=), else $XDG_CONFIG_HOME/guest-agent, else $HOME/.config/guest-agent then
   /etc/guest-agent. Phase dir = root + dir name bellow.
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
 - `MOUNT_POINT`: The mount point of the snapshot (e.g., "/dev/xvdb").

Each hook is executed once per snapshot created in the group. If a hook interacts with an external process (e.g., a database), it must be idempotent where necessary. The total execution time of all hooks in the group must not exceed 30 seconds. Hooks that exceed this limit will be terminated with a SIGKILL signal.

## Exit Codes

Hooks should return exit code 0 for success. Non-zero exit codes will cause the snapshot operation to be considered `crash-consistent`, and the error will be logged.
