use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::Duration,
};

use log::{debug, error, info, warn};

const DEFAULT_TIMEOUT: u64 = 30_000;
const REAP_GRACE: Duration = Duration::from_secs(5);

#[derive(serde::Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub volume_id: String,
    pub mount_point: String,
    pub success: Option<bool>,
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
    Begin,
    Commit,
    Completed,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Commit => "commit",
            Self::Completed => "completed",
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Self::Begin => "snapshot.d",
            Self::Commit => "snapshot-commit.d",
            Self::Completed => "snapshot-completed.d",
        }
    }
}

pub fn configuration_dir_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    //systemd Service
    if let Ok(dir) = std::env::var("CONFIGURATION_DIRECTORY") {
        for part in dir.split(':').map(str::trim).filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(part));
        }
    }

    //XDG
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        dirs.push(PathBuf::from(xdg).join("guest-agent"));
    }

    //Fallback
    if dirs.is_empty() {
        if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            dirs.push(PathBuf::from(home).join(".config/guest-agent"));
        }
        dirs.push(PathBuf::from("/etc/guest-agent"));
    }

    dirs
}

fn is_run_parts_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn collect_hooks(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut hooks = Vec::new();
    for ent in fs::read_dir(dir)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_run_parts_name(name) {
            debug!(target: "guest_agent::snapshot", "skip non run-parts name: {name}");
            continue;
        }
        if !is_executable(&path) {
            debug!(target: "guest_agent::snapshot", "skip non-executable: {}", path.display());
            continue;
        }
        hooks.push(path);
    }
    hooks.sort();
    Ok(hooks)
}

fn drain_stdio(pipe: impl Read, phase: &str, script: &str, stderr: bool) {
    let reader = BufReader::new(pipe);
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.is_empty() {
            continue;
        }
        if stderr {
            warn!(
                target: "guest_agent::snapshot",
                "hook stderr phase={phase} script={script}: {line}"
            );
        } else {
            debug!(
                target: "guest_agent::snapshot",
                "hook stdout phase={phase} script={script}: {line}"
            );
        }
    }
}

fn run_hook_script(
    path: &Path,
    phase: Phase,
    env: &Snapshot,
    deadline: jiff::Timestamp,
) -> Result<(), String> {
    info!(
        target: "guest_agent::snapshot",
        "hook start phase={} script={}",
        phase.as_str(),
        path.display()
    );

    let mut cmd = Command::new(path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PHASE", phase.as_str())
        .env("VOLUME_ID", &env.volume_id)
        .env("SNAPSHOT_ID", &env.id)
        .env("MOUNT_POINT", &env.mount_point);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", path.display()))?;

    let phase_s = phase.as_str().to_string();
    let script = path.display().to_string();
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let out_phase = phase_s.clone();
    let out_script = script.clone();
    let err_phase = phase_s;
    let err_script = script;
    let _ = thread::spawn(move || drain_stdio(stdout, &out_phase, &out_script, false));
    let _ = thread::spawn(move || drain_stdio(stderr, &err_phase, &err_script, true));

    let status = wait_child_deadline(child, deadline, path)?;

    if status.success() {
        info!(
            target: "guest_agent::snapshot",
            "hook ok phase={} script={}",
            phase.as_str(),
            path.display()
        );
        Ok(())
    } else {
        Err(format!(
            "hook failed exit_code={} phase={} script={}",
            status.code().unwrap_or_default(),
            phase.as_str(),
            path.display()
        ))
    }
}

fn wait_child_deadline(
    mut child: Child,
    limit: jiff::Timestamp,
    path: &Path,
) -> Result<ExitStatus, String> {
    loop {
        let now = jiff::Timestamp::now();

        match child
            .try_wait()
            .map_err(|e| format!("wait {}: {e}", path.display()))?
        {
            Some(status) => return Ok(status),
            None if now >= limit => break,
            None => thread::sleep(Duration::from_millis(20)),
        }
    }

    warn!(
        target: "guest_agent::snapshot",
        "hook timeout, killing {}",
        path.display()
    );
    if let Err(e) = child.kill() {
        // ESRCH here just means the child exited between try_wait and kill.
        debug!(
            target: "guest_agent::snapshot",
            "hook kill {} failed: {e}",
            path.display()
        );
    }

    let reap_by = jiff::Timestamp::now() + REAP_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!(
                    "hook {} timed out (killed, {status})",
                    path.display(),
                ));
            }
            Ok(None) if jiff::Timestamp::now() >= reap_by => {
                return Err(format!("hook {} timed out (zombie)", path.display()));
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                error!(target: "guest_agent::snapshot", "hook {} reap error: {e}", path.display());
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

pub fn run_hooks(
    phase: Phase,
    env: &Snapshot,
    deadline: Option<jiff::Timestamp>,
) -> Result<(), String> {
    let dir = configuration_dir_candidates()
        .iter()
        .map(|d| d.join(phase.dir_name()))
        .find(|d| d.exists());

    let Some(dir) = dir else {
        debug!(
            target: "guest_agent::snapshot",
            "no hooks dir phase={} (ok)",
            phase.as_str()
        );
        return Ok(());
    };

    let hooks = collect_hooks(&dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    let deadline =
        deadline.unwrap_or_else(|| jiff::Timestamp::now() + Duration::from_millis(DEFAULT_TIMEOUT));

    if hooks.is_empty() {
        debug!(
            target: "guest_agent::snapshot",
            "no hooks phase={} dir={}",
            phase.as_str(),
            dir.display()
        );
        return Ok(());
    }

    info!(
        target: "guest_agent::snapshot",
        "running {} hook(s) phase={} dir={}",
        hooks.len(),
        phase.as_str(),
        dir.display()
    );

    let mut failures = Vec::new();
    for hook in hooks {
        if let Err(err) = run_hook_script(&hook, phase, env, deadline) {
            failures.push(err);
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        // Hooks are best-effort per hook but the phase as a whole must report
        // failure: the host snapshots based on this reply.
        Err(failures.join("; "))
    }
}

pub fn begin(env: &Snapshot, deadline: String) -> Result<(), String> {
    let deadline: Option<jiff::Timestamp> = match deadline.parse() {
        Ok(t) => Some(t),
        Err(e) => {
            error!(target: "guest_agent::snapshot", "snapshot_begin: could not parse deadline: {e}");
            None
        }
    };

    run_hooks(Phase::Begin, env, deadline).inspect_err(
        |e| error!(target: "guest_agent::snapshot", "snapshot_begin hooks failed: {e}"),
    )
}

pub fn commit(env: &Snapshot) -> Result<(), String> {
    run_hooks(Phase::Commit, env, None).inspect_err(
        |e| error!(target: "guest_agent::snapshot", "snapshot_commit hooks failed: {e}"),
    )
}

pub fn completed(env: &Snapshot) -> Result<(), String> {
    run_hooks(Phase::Completed, env, None).inspect_err(
        |e| error!(target: "guest_agent::snapshot", "snapshot_completed hooks failed: {e}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn snapshot() -> Snapshot {
        Snapshot {
            id: "snap-test".into(),
            volume_id: "vol-test".into(),
            mount_point: "/dev/null".into(),
            success: None,
        }
    }

    fn config_dir(name: &str) -> PathBuf {
        let tmp =
            std::env::temp_dir().join(format!("guest-agent-hooks-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&tmp));
        fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    fn set_config_dir(tmp: &Path) {
        // SAFETY: tests hold ENV_LOCK around env access, so no other test
        // thread reads the environment while it is mutated.
        unsafe { std::env::set_var("CONFIGURATION_DIRECTORY", tmp) };
        // SAFETY: same ENV_LOCK exclusion as above.
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    fn unset_config_dir() {
        // SAFETY: tests hold ENV_LOCK around env access, so no other test
        // thread reads the environment while it is mutated.
        unsafe { std::env::remove_var("CONFIGURATION_DIRECTORY") };
        // SAFETY: same ENV_LOCK exclusion as above.
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    #[test]
    fn hook_failure_is_reported() {
        let _g = ENV_LOCK.lock().unwrap();
        let tmp = config_dir("fail");
        let hook_dir = tmp.join("snapshot.d");
        fs::create_dir_all(&hook_dir).unwrap();
        let hook = hook_dir.join("10-fail");
        fs::write(&hook, "#!/bin/sh\nexit 3\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

        set_config_dir(&tmp);
        let err = run_hooks(Phase::Begin, &snapshot(), None).unwrap_err();
        unset_config_dir();
        drop(fs::remove_dir_all(&tmp));

        assert!(err.contains("10-fail"), "{err}");
    }

    #[test]
    fn missing_phase_dir_is_not_an_error() {
        let _g = ENV_LOCK.lock().unwrap();
        let tmp = config_dir("nophase");

        set_config_dir(&tmp);
        run_hooks(Phase::Commit, &snapshot(), None).unwrap();
        unset_config_dir();
        drop(fs::remove_dir_all(&tmp));
    }
}
