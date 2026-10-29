use std::{borrow::Cow, thread, time::Duration};

use log::{debug, error, info, warn};

const ZAP_ENDPOINT: &str = "inproc://zeromq.zap.01";
const ZAP_DOMAIN: &str = "*";

/// Fatal setup error: log and exit so the supervisor restarts the agent.
fn die(action: &str, name: &str, e: impl std::fmt::Display) -> ! {
    error!(target: "guest_agent::zmq", "{action} {name}: {e}");
    std::process::exit(1);
}

#[derive(Clone, Copy)]
enum Listener {
    Pubkey,
    Healthcheck,
    Rpc,
}

impl Listener {
    fn name(self) -> &'static str {
        match self {
            Self::Pubkey => "pubkey-rep",
            Self::Healthcheck => "healthcheck-rep",
            Self::Rpc => "rpc-router",
        }
    }

    fn endpoint(self) -> Cow<'static, str> {
        #[cfg(debug_assertions)]
        {
            let variable = match self {
                Self::Pubkey => "GUEST_AGENT_PUBKEY_ENDPOINT",
                Self::Healthcheck => "GUEST_AGENT_HEALTHCHECK_ENDPOINT",
                Self::Rpc => "GUEST_AGENT_RPC_ENDPOINT",
            };
            if let Ok(value) = std::env::var(variable)
                && !value.is_empty()
            {
                return Cow::Owned(value);
            }
        }
        Cow::Borrowed(match self {
            Self::Pubkey => "vsock://*:8000",
            Self::Healthcheck => "vsock://*:8001",
            Self::Rpc => "vsock://*:8002",
        })
    }

    fn open(
        self,
        ctx: &zmq::ZmqContext,
        key: &guest_agent::curve::CurveKeyPair,
    ) -> Result<zmq::ZmqSocket, zmq::ZmqError> {
        let (socket_type, identity): (_, &[u8]) = match self {
            Self::Pubkey | Self::Healthcheck => (zmq::ZMQ_REP, b"osc-agent-guest-pubkey"),
            Self::Rpc => (zmq::ZMQ_ROUTER, b"osc-agent-guest-router"),
        };
        let sock = ctx.socket(socket_type)?;
        sock.set_identity(identity)?;
        sock.set_linger(0)?;
        sock.set_sndtimeo(300)?;
        match self {
            Self::Pubkey => {}
            Self::Healthcheck => {
                sock.set_zap_domain(ZAP_DOMAIN)?;
                sock.set_curve_server(key.secret.as_str(), key.public.as_str())?;
            }
            Self::Rpc => {
                sock.set_sndhwm(1)?;
                sock.set_zap_domain(ZAP_DOMAIN)?;
                sock.set_curve_server(key.secret.as_str(), key.public.as_str())?;
            }
        }
        let endpoint = self.endpoint();
        sock.bind(&endpoint)?;
        info!(target: "guest_agent::zmq", "{} listening on {endpoint}", self.name());
        Ok(sock)
    }
}

fn serve_rep(
    listener: Listener,
    ctx: &zmq::ZmqContext,
    key: &guest_agent::curve::CurveKeyPair,
    handle_rpc: fn(&[u8]) -> Option<Vec<u8>>,
) {
    let name = listener.name();
    loop {
        let sock = listener
            .open(ctx, key)
            .unwrap_or_else(|e| die("open", name, e));
        loop {
            let frames = match sock.recv_multipart(0) {
                Ok(f) => f,
                Err(e) => {
                    error!(target: "guest_agent::zmq", "{name} recv: {e}");
                    break;
                }
            };

            let payload = frames.last().map(Vec::as_slice).unwrap_or(&[]);
            // Notifications get an empty REP frame: a REP socket must reply
            // before its next recv or it wedges in EFSM forever.
            let reply = handle_rpc(payload).unwrap_or_default();

            match sock.send_multipart(&[reply.as_slice()], 0) {
                Ok(()) => {}
                Err(e) if e.is_eagain() => {
                    warn!(target: "guest_agent::zmq", "{name} send EAGAIN — recreating socket");
                    break;
                }
                Err(e) => {
                    error!(target: "guest_agent::zmq", "{name} send failed ({e}) — recreating socket");
                    break;
                }
            }
        }
        // Drop the REP socket before rebinding after a failed exchange.
    }
}

fn serve_router(sock: zmq::ZmqSocket, handle_rpc: fn(&[u8]) -> Option<Vec<u8>>) {
    loop {
        match sock.poll(zmq::ZMQ_POLLIN, -1) {
            Ok(revents) if revents & zmq::ZMQ_POLLIN != 0 => {}
            Ok(_) => continue,
            Err(e) if e.is_eagain() => continue,
            Err(e) => {
                error!(target: "guest_agent::zmq", "rpc ROUTER poll: {e}");
                continue;
            }
        }

        let frames = match sock.recv_multipart(0) {
            Ok(f) => f,
            Err(e) if e.is_eagain() => continue,
            Err(e) => {
                error!(target: "guest_agent::zmq", "rpc ROUTER recv: {e}");
                continue;
            }
        };

        // ROUTER envelope: [routing_id, ...payload]
        let Some(routing_id) = frames.first() else {
            warn!(target: "guest_agent::zmq", "rpc ROUTER empty multipart");
            continue;
        };
        let payload = frames.last().map(Vec::as_slice).unwrap_or(&[]);
        debug!(
            target: "guest_agent::zmq",
            "rpc ROUTER request frames={} routing_id_len={}",
            frames.len(),
            routing_id.len()
        );
        let Some(reply) = handle_rpc(payload) else {
            continue;
        };

        match sock.send_multipart(&[routing_id.as_slice(), reply.as_slice()], 0) {
            Ok(()) => {}
            Err(e) if e.is_eagain() => {
                warn!(target: "guest_agent::zmq", "rpc ROUTER send EAGAIN — dropping reply");
                continue;
            }
            Err(e) => {
                error!(target: "guest_agent::zmq", "rpc ROUTER send: {e}");
                continue;
            }
        }
    }
}

fn open_zap(ctx: &zmq::ZmqContext) -> Result<zmq::ZmqSocket, zmq::ZmqError> {
    let sock = ctx.socket(zmq::ZMQ_REP)?;
    sock.set_linger(0)?;
    sock.bind(ZAP_ENDPOINT)?;
    Ok(sock)
}

fn serve_zap(sock: zmq::ZmqSocket) {
    loop {
        let frames = match sock.recv_multipart(0) {
            Ok(f) => f,
            Err(e) => {
                error!(target: "guest_agent::zmq", "zap recv: {e}");
                continue;
            }
        };
        let request_id = frames.get(1).cloned().unwrap_or_default();
        let status = zap_auth(&frames);

        let reply: [&[u8]; 6] = [b"1.0", request_id.as_slice(), status, b"", b"", b""];
        if let Err(e) = sock.send_multipart(&reply, 0) {
            error!(target: "guest_agent::zmq", "zap send: {e}");
        }
    }
}

fn zap_auth(frames: &[Vec<u8>]) -> &'static [u8] {
    if frames.first().map(Vec::as_slice) != Some(b"1.0".as_slice()) {
        return b"500";
    }
    if frames.get(5).map(Vec::as_slice) != Some(b"CURVE".as_slice()) {
        let dump: Vec<String> = frames
            .iter()
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .collect();
        warn!(target: "guest_agent::zmq", "zap: unexpected mechanism, denying ({dump:?})");
        return b"400";
    }
    let Some(credential) = frames.get(6) else {
        warn!(target: "guest_agent::zmq", "zap: missing CURVE credential, denying");
        return b"400";
    };
    let Ok(client_key) = zmq::z85_encode(credential) else {
        warn!(target: "guest_agent::zmq", "zap: malformed CURVE credential, denying");
        return b"400";
    };

    if crate::HOST_PUBLIC_KEY.get().is_none()
        && let Some(key) = guest_agent::curve::fetch_metadata_curve()
            .ok()
            .and_then(|m| m.peer_public_key)
    {
        warn!(target: "guest_agent::zmq", "zap: pinned host key {key} (metadata retry)");
        if crate::HOST_PUBLIC_KEY.set(key).is_err() {
            debug!(target: "guest_agent::zmq", "zap: host key already pinned");
        }
    }

    match crate::HOST_PUBLIC_KEY.get() {
        Some(pinned) if pinned == &client_key => b"200",
        Some(pinned) => {
            warn!(target: "guest_agent::zmq", "zap: rejecting client key {client_key}, pinned {pinned}");
            b"400"
        }
        None => {
            warn!(target: "guest_agent::zmq", "zap: no host key available, allowing client {client_key}");
            b"200"
        }
    }
}

pub(super) fn run(
    ctx: zmq::ZmqContext,
    key: &guest_agent::curve::CurveKeyPair,
    handle_rpc: fn(&[u8]) -> Option<Vec<u8>>,
    handle_bootstrap: fn(&[u8]) -> Option<Vec<u8>>,
) -> ! {
    // ZAP handler must be bound before any CURVE socket accepts connections.
    let zap_sock = open_zap(&ctx).unwrap_or_else(|e| die("open", "zap", e));
    let mut threads = vec![
        thread::Builder::new()
            .name("zap-auth".into())
            .spawn(move || serve_zap(zap_sock))
            .unwrap_or_else(|e| die("spawn", "zap-auth", e)),
    ];

    for listener in [Listener::Pubkey, Listener::Healthcheck, Listener::Rpc] {
        let name = listener.name();
        let ctx = ctx.clone();
        let key_clone = key.clone();
        let handler = match listener {
            Listener::Pubkey => handle_bootstrap,
            Listener::Healthcheck | Listener::Rpc => handle_rpc,
        };
        threads.push(
            thread::Builder::new()
                .name(name.into())
                .spawn(move || match listener {
                    Listener::Pubkey | Listener::Healthcheck => {
                        serve_rep(listener, &ctx, &key_clone, handler)
                    }
                    Listener::Rpc => {
                        let sock = listener
                            .open(&ctx, &key_clone)
                            .unwrap_or_else(|e| die("open", name, e));
                        serve_router(sock, handler);
                    }
                })
                .unwrap_or_else(|e| die("spawn", name, e)),
        );
    }
    info!(target: "guest_agent", "listeners running");

    let origin = 'outer: loop {
        for (i, handle) in threads.iter().enumerate() {
            if handle.is_finished() {
                break 'outer i;
            }
        }
        thread::sleep(Duration::from_millis(20));
    };
    let handle = threads
        .into_iter()
        .nth(origin)
        .expect("origin was produced by enumerating threads");
    let name = handle.thread().name().unwrap_or_default().to_owned();
    if let Err(e) = handle.join() {
        error!(target: "guest_agent", "thread {name} panicked: {e:?}");
    } else {
        error!(target: "guest_agent", "thread {name} exited unexpectedly");
    }
    std::process::exit(1);
}
