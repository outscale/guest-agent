# shellcheck shell=bash
# Shared helpers for snapshot hooks.
# Env from guest-agent (overridable by the hook before helpers run):
#   MOUNT_POINT  block device, e.g. /dev/xvdh  (wire name is historical)
#   VOLUME_ID SNAPSHOT_ID 
#   PHASE        begin | commit | completed
#   CONTAINER_RUNTIME_ENDPOINT  optional (default containerd sock)
#
# Annotation argv may use ${VOLUME_ID} ${SNAPSHOT_ID} ${MOUNT_POINT} ${PHASE}
# ${FREEZE_TARGET} ${POD_UID}. Same names are forwarded into
# the container process via `env` on crictl exec.

: "${MOUNT_POINT:?MOUNT_POINT (device) required}"
: "${PHASE:=begin}"

export CONTAINER_RUNTIME_ENDPOINT="${CONTAINER_RUNTIME_ENDPOINT:-unix:///run/containerd/containerd.sock}"

ANN_BEGIN='snapshot.outscale.com/begin-command'
ANN_COMMIT='snapshot.outscale.com/commit-command'
ANN_COMPLETED='snapshot.outscale.com/completed-command'
ANN_CONTAINER='snapshot.outscale.com/container'

log() { echo "snapshot-hook[${PHASE}]: $*" >&2; }

# Names expanded in annotation argv and forwarded into container env.
HOOK_VARS=(VOLUME_ID SNAPSHOT_ID MOUNT_POINT PHASE FREEZE_TARGET POD_UID)

# ${NAME} → value for HOOK_VARS (unknown ${...} left intact).
hook_expand() {
  local s=$1 n
  for n in "${HOOK_VARS[@]}"; do
    s=${s//\$\{$n\}/${!n-}}
  done
  printf '%s' "$s"
}

# stdin lines → stdout, each line hook_expand'd.
hook_expand_lines() {
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    hook_expand "$line"
    printf '\n'
  done
}

# argv for `env KEY=VAL …` from HOOK_VARS (skip unset/empty).
hook_env_args() {
  local n
  for n in "${HOOK_VARS[@]}"; do
    [[ -n "${!n-}" ]] || continue
    printf '%s=%s\n' "$n" "${!n}"
  done
}

# Canonical device node (resolve by-id symlinks).
device_path() {
  local d
  d=$(readlink -f "$MOUNT_POINT" 2>/dev/null || true)
  if [[ -z "$d" || ! -b "$d" ]]; then
    log "not a block device: $MOUNT_POINT"
    return 1
  fi
  printf '%s\n' "$d"
}

# Mount targets whose SOURCE is this device (or a partition of it).
mount_targets_for_device() {
  local dev kid
  dev=$(device_path) || return 1

  findmnt -rn -S "$dev" -o TARGET 2>/dev/null || true

  # disk node with fs on a child partition
  while read -r kid; do
    [[ -n "$kid" && -b "$kid" ]] || continue
    findmnt -rn -S "$kid" -o TARGET 2>/dev/null || true
  done < <(lsblk -rn -o PATH,TYPE "$dev" 2>/dev/null | awk '$2 != "disk" { print $1 }')
}

# Prefer kubelet CSI pod bind-mount; else first mount.
# Sets: FREEZE_TARGET POD_UID
resolve_mount_and_pod() {
  FREEZE_TARGET=""
  POD_UID=""

  local t uid
  local -a targets=()
  mapfile -t targets < <(mount_targets_for_device | awk 'NF && !seen[$0]++')

  for t in "${targets[@]:-}"; do
    if [[ "$t" =~ /pods/([^/]+)/volumes/ ]]; then
      POD_UID="${BASH_REMATCH[1]}"
      FREEZE_TARGET="$t"
      return 0
    fi
  done

  if [[ ${#targets[@]} -gt 0 ]]; then
    FREEZE_TARGET="${targets[0]}"
  fi
  return 0
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    log "missing command: $1"
    return 1
  }
}

# crictl sandbox id for pod uid → stdout
sandbox_for_pod() {
  local uid=$1
  need_cmd crictl || return 1
  crictl pods --label io.kubernetes.pod.uid="$uid" \
    --state Ready -o json | jq -r '.items[0].id // empty' 2>/dev/null
}

annotation() {
  local sb=$1 key=$2
  need_cmd jq || return 1
  crictl inspectp "$sb" | jq -r --arg k "$key" '.status.annotations[$k] // empty'
}

# JSON argv array → exec in container. No-op if annotation absent.
pod_exec_annotation() {
  local key=$1
  local sb ctr cname cmd
  local -a args=()

  [[ -n "${POD_UID:-}" ]] || {
    log "no pod uid for device (skip exec)"
    return 0
  }

  need_cmd crictl || return 1
  need_cmd jq || return 1

  sb=$(sandbox_for_pod "$POD_UID")
  [[ -n "$sb" ]] || {
    log "no CRI sandbox for pod uid=$POD_UID"
    return 1
  }

  cmd=$(annotation "$sb" "$key")
  [[ -n "$cmd" ]] || {
    log "no annotation $key (skip exec)"
    return 0
  }

  cname=$(annotation "$sb" "$ANN_CONTAINER")
  ctr=$(
    crictl ps --pod "$sb" -o json \
      | jq -r --arg n "${cname:-}" '
          .containers
          | (if $n != "" then map(select(.metadata.name == $n)) else . end)
          | .[0].id // empty'
  )
  [[ -n "$ctr" ]] || {
    log "no container id (name=${cname:-first})"
    return 1
  }

  mapfile -t args < <(jq -r '.[]' <<<"$cmd" | hook_expand_lines)
  [[ ${#args[@]} -gt 0 ]] || {
    log "empty command array in $key"
    return 1
  }

  local -a env_args=()
  mapfile -t env_args < <(hook_env_args)

  log "crictl exec container=$ctr env=${env_args[*]} cmd=${args[*]}"
  if [[ ${#env_args[@]} -gt 0 ]]; then
    crictl exec "$ctr" env "${env_args[@]}" "${args[@]}"
  else
    crictl exec "$ctr" "${args[@]}"
  fi
}

fs_freeze() {
  [[ -n "${FREEZE_TARGET:-}" ]] || {
    log "no filesystem mount for device (skip freeze)"
    return 0
  }
  need_cmd fsfreeze || return 1
  log "fsfreeze --freeze $FREEZE_TARGET"
  fsfreeze --freeze "$FREEZE_TARGET"
}

fs_unfreeze() {
  [[ -n "${FREEZE_TARGET:-}" ]] || {
    log "no filesystem mount for device (skip unfreeze)"
    return 0
  }
  need_cmd fsfreeze || return 1
  log "fsfreeze --unfreeze $FREEZE_TARGET"
  # already thawed is ok
  fsfreeze --unfreeze "$FREEZE_TARGET" 2>/dev/null || true
}
