use std::sync::OnceLock;

use guest_agent::{
    rpc_models::{Request, Response, RpcError},
    snapshot_hooks::Snapshot,
};
use log::{debug, error, info, warn};
use serde_json::Value;

mod logger;
mod zmq_server;

static SERVER_PUBLIC_KEY: OnceLock<String> = OnceLock::new();
static HOST_PUBLIC_KEY: OnceLock<String> = OnceLock::new();

#[derive(serde::Serialize)]
struct HandshakeResponse {
    guest_public_key: String,
    version: String,
}

#[guest_agent_macros::json_rpc_method]
fn get_pub_key(host_public_key: String, version: String) -> HandshakeResponse {
    const VERSION: &str = "v1.0";

    if let Some(key) = HOST_PUBLIC_KEY.get()
        && key != &host_public_key
    {
        warn!(target: "guest_agent::rpc", "host public key mismatch: expected {} got {}", key, host_public_key);
    }

    if version != VERSION {
        warn!(target: "guest_agent::rpc", "host version mismatch: expected {} got {}", VERSION, version);
    }

    HandshakeResponse {
        guest_public_key: SERVER_PUBLIC_KEY.get().cloned().unwrap_or_default(),
        version: VERSION.to_string(),
    }
}

#[guest_agent_macros::json_rpc_method]
fn get_heartbeat() -> bool {
    true
}

#[guest_agent_macros::json_rpc_method]
fn snapshot_begin(
    snapshots: Vec<Snapshot>,
    deadline: String,
    request_id: String,
) -> Result<bool, String> {
    for snap in snapshots.iter() {
        info!(
            target: "guest_agent::snapshot",
            "snapshot_begin volume_id={} snapshot_id={} deadline={} mount_point={} request_id={request_id}",
            snap.volume_id,
            snap.id,
            deadline,
            snap.mount_point,
        );

        guest_agent::snapshot_hooks::begin(snap, deadline.clone())?;
    }

    Ok(true)
}

#[guest_agent_macros::json_rpc_method]
fn snapshot_commit(snapshots: Vec<Snapshot>, request_id: String) -> Result<bool, String> {
    for snap in snapshots.iter() {
        info!(
            target: "guest_agent::snapshot",
            "snapshot_commit volume_id={} snapshot_id={} request_id={request_id}",
            snap.volume_id,
            snap.id,
        );
        guest_agent::snapshot_hooks::commit(snap)?;
    }

    Ok(true)
}

#[guest_agent_macros::json_rpc_method]
fn snapshot_completed(
    snapshots: Vec<Snapshot>,
    mode: String,
    request_id: String,
) -> Result<bool, String> {
    for snap in snapshots.iter() {
        info!(
            target: "guest_agent::snapshot",
            "snapshot_completed volume_id={} snapshot_id={} success={} mode={mode} request_id={request_id}",
            snap.volume_id,
            snap.id,
            snap.success.unwrap_or_default(),
        );
        guest_agent::snapshot_hooks::completed(snap)?;
    }

    Ok(true)
}

fn parse_error_reply() -> Vec<u8> {
    serde_json::to_vec(&Response::error(Value::Null, RpcError::parse_error()))
        .expect("parse-error reply serializes")
}

fn internal_error_reply(id: Value) -> Vec<u8> {
    serde_json::to_vec(&Response::error(id, RpcError::internal_error()))
        .expect("internal-error reply serializes")
}

const BOOTSTRAP_METHODS: &[&str] = &["get_pub_key", "get_heartbeat"];
fn handle_rpc_handshake(payload: &[u8]) -> Option<Vec<u8>> {
    match serde_json::from_slice::<Request>(payload) {
        Ok(req) if BOOTSTRAP_METHODS.contains(&req.method.as_str()) => handle_rpc(payload),
        Ok(req) => {
            warn!(
                target: "guest_agent::rpc",
                "method {} denied on plaintext listener",
                req.method
            );
            req.id.map(|id| {
                serde_json::to_vec(&Response::error(id, RpcError::method_not_found()))
                    .unwrap_or_else(|_| parse_error_reply())
            })
        }
        Err(_) => Some(parse_error_reply()),
    }
}

fn handle_rpc(payload: &[u8]) -> Option<Vec<u8>> {
    let registry = guest_agent::registry::JsonRpcMethodRegistry::global();
    match serde_json::from_slice::<Request>(payload) {
        Ok(req) => {
            let id = req.id.clone().unwrap_or(Value::Null);
            debug!(
                target: "guest_agent::rpc",
                "request method={} id={id}",
                req.method
            );
            match registry.dispatch(&req) {
                Some(response) => Some(serde_json::to_vec(&response).unwrap_or_else(|e| {
                    error!(
                        target: "guest_agent::rpc",
                        "serialize response failed id={id}: {e}"
                    );
                    internal_error_reply(id)
                })),
                None => {
                    debug!(
                        target: "guest_agent::rpc",
                        "notification method={} id=-",
                        req.method
                    );
                    None
                }
            }
        }
        Err(e) => {
            warn!(target: "guest_agent::rpc", "parse error: {e}");
            Some(parse_error_reply())
        }
    }
}

fn main() {
    logger::setup_logger();

    info!(target: "guest_agent", "starting osc-agent-guest");

    let _ = guest_agent::registry::JsonRpcMethodRegistry::global();
    let keys = guest_agent::curve::load_or_create_agent_keys().expect("agent CURVE keys");
    info!(
        target: "guest_agent::curve",
        "agent CURVE public key ready",
    );

    match guest_agent::curve::fetch_metadata_curve() {
        Ok(m) => {
            info!(
                target: "guest_agent::curve",
                "metadata CURVE peer key loaded",
            );
            if let Some(key) = &m.peer_public_key
                && HOST_PUBLIC_KEY.set(key.clone()).is_err()
            {
                debug!(
                    target: "guest_agent::curve",
                    "host public key already pinned"
                );
            }
        }
        Err(e) => debug!(target: "guest_agent::curve", "metadata CURVE fetch: {e}"),
    }

    SERVER_PUBLIC_KEY
        .set(keys.public.clone())
        .expect("public key already set");

    let ctx = zmq::ZmqContext::new().expect("ZMQ context");
    info!(target: "guest_agent::zmq", "ZMQ context created");
    zmq_server::run(ctx, &keys, handle_rpc, handle_rpc_handshake);
}
