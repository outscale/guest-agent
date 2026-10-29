use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use log::{debug, info, warn};

const PUBLIC_FILE: &str = "curve.public";
const SECRET_FILE: &str = "curve.secret";
const KEY_MODE: u32 = 0o600;

#[derive(Debug, Clone)]
pub struct CurveKeyPair {
    pub public: String,
    pub secret: String,
}

pub fn state_dir_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    //systemd Service
    if let Ok(dir) = std::env::var("STATE_DIRECTORY") {
        for part in dir.split(':').map(str::trim).filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(part));
        }
    }

    //XDG
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME")
        && !xdg.is_empty()
    {
        dirs.push(PathBuf::from(xdg).join("guest-agent"));
    }

    //Fallback
    if dirs.is_empty() {
        if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            dirs.push(PathBuf::from(home).join(".local/state/guest-agent"));
        }
        dirs.push(PathBuf::from("/var/lib/guest-agent"));
    }

    dirs
}

pub fn load_or_create_agent_keys() -> Result<CurveKeyPair, String> {
    let mut errors = Vec::new();
    let candidates = state_dir_candidates();

    debug!(
        target: "guest_agent::curve",
        "state dir candidates: {:?}",
        candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
    );

    //Look for existing keys
    for dir in candidates.iter() {
        let public_path = dir.join(PUBLIC_FILE);
        let secret_path = dir.join(SECRET_FILE);

        if public_path.exists() && secret_path.exists() {
            match (read_key_file(&public_path), read_key_file(&secret_path)) {
                (Ok(public), Ok(secret)) => {
                    info!(
                        target: "guest_agent::curve",
                        "loaded agent CURVE keys from {}",
                        dir.display()
                    );
                    return Ok(CurveKeyPair { public, secret });
                }
                (Err(e), _) | (_, Err(e)) => {
                    warn!(
                        target: "guest_agent::curve",
                        "failed reading keys in {}: {e}",
                        dir.display()
                    );
                    errors.push(e);
                }
            }
        }
    }

    //Find a suitable state dir and create new keys
    for dir in candidates.iter() {
        let public_path = dir.join(PUBLIC_FILE);
        let secret_path = dir.join(SECRET_FILE);

        if let Err(e) = fs::create_dir_all(dir) {
            debug!(
                target: "guest_agent::curve",
                "skip state dir {}: mkdir failed: {e}",
                dir.display()
            );
            errors.push(format!("mkdir {}: {e}", dir.display()));
            continue;
        }

        let (public, secret) = match zmq::zmq_curve_keypair() {
            Ok(k) => k,
            Err(e) => return Err(format!("generate CURVE keypair: {e}")),
        };

        if let Err(e) = write_key_file(&public_path, &public) {
            warn!(
                target: "guest_agent::curve",
                "failed writing public key to {}: {e}",
                public_path.display()
            );
            errors.push(e);
            continue;
        }
        if let Err(e) = write_key_file(&secret_path, &secret) {
            if let Err(rm) = fs::remove_file(&public_path) {
                warn!(
                    target: "guest_agent::curve",
                    "failed removing partial public key {}: {rm}",
                    public_path.display()
                );
            }
            warn!(
                target: "guest_agent::curve",
                "failed writing secret key to {}: {e}",
                secret_path.display()
            );
            errors.push(e);
            continue;
        }
        info!(
            target: "guest_agent::curve",
            "generated and stored agent CURVE keys in {}",
            dir.display()
        );
        return Ok(CurveKeyPair { public, secret });
    }

    Err(format!(
        "agent CURVE keys: no usable state directory: {}",
        errors.join("; ")
    ))
}

fn read_key_file(path: &Path) -> Result<String, String> {
    let mut f = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let key = buf.trim().to_owned();
    if key.is_empty() {
        return Err(format!("empty key file {}", path.display()));
    }

    // enforce 0o600 on existing files too
    if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(KEY_MODE)) {
        warn!(
            target: "guest_agent::curve",
            "failed enforcing key mode on {}: {e}",
            path.display()
        );
    }
    Ok(key)
}

fn write_key_file(path: &Path, key: &str) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(KEY_MODE)
        .open(path)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    writeln!(f, "{key}").map_err(|e| format!("write {}: {e}", path.display()))?;
    f.sync_all()
        .map_err(|e| format!("sync {}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(KEY_MODE))
        .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    Ok(())
}

const METADATA_CURVE_HOST: &str = "169.254.169.254";
const METADATA_CURVE_PORT: u16 = 80;
const METADATA_CURVE_PATH: &str = "/latest/meta-data/host-public-key";
const METADATA_TIMEOUT: Duration = Duration::from_secs(2);
const Z85_KEY_LEN: usize = 40;

pub struct MetadataCurve {
    pub peer_public_key: Option<String>,
}

/// Fetch client CURVE public key from
/// `http://169.254.169.254/latest/meta-data/host-public-key`.
///
/// Debug builds: override host:port with `GUEST_AGENT_METADATA_ADDR`.
pub fn fetch_metadata_curve() -> Result<MetadataCurve, String> {
    let (host, port) = metadata_addr();
    let body = http_get_text(&host, port, METADATA_CURVE_PATH)?;
    let key = parse_z85_public_key(&body)?;
    Ok(MetadataCurve {
        peer_public_key: Some(key),
    })
}

fn metadata_addr() -> (String, u16) {
    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var("GUEST_AGENT_METADATA_ADDR") {
        if let Some((h, p)) = raw.rsplit_once(':')
            && let Ok(port) = p.parse()
        {
            return (h.to_owned(), port);
        }
        warn!(
            target: "guest_agent::curve",
            "invalid GUEST_AGENT_METADATA_ADDR={raw:?}, using default"
        );
    }
    (METADATA_CURVE_HOST.to_owned(), METADATA_CURVE_PORT)
}

fn http_get_text(host: &str, port: u16, path: &str) -> Result<String, String> {
    let addr = resolve_addr(host, port)?;
    let mut stream = TcpStream::connect_timeout(&addr, METADATA_TIMEOUT)
        .map_err(|e| format!("metadata connect {host}:{port}: {e}"))?;
    stream
        .set_read_timeout(Some(METADATA_TIMEOUT))
        .map_err(|e| format!("metadata read timeout: {e}"))?;
    stream
        .set_write_timeout(Some(METADATA_TIMEOUT))
        .map_err(|e| format!("metadata write timeout: {e}"))?;

    let req = format!("GET {path} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("metadata write: {e}"))?;

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| format!("metadata read: {e}"))?;
    let raw = String::from_utf8(buf).map_err(|e| format!("metadata body not utf-8: {e}"))?;
    parse_http_body(&raw)
}

fn resolve_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("metadata resolve {host}:{port}: {e}"))?
        .next()
        .ok_or_else(|| format!("metadata resolve {host}:{port}: no address"))
}

fn parse_http_body(raw: &str) -> Result<String, String> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .ok_or("metadata response missing header terminator")?;

    let status_line = head.lines().next().unwrap_or("");
    // "HTTP/1.0 200 OK" / "HTTP/1.1 200 OK"
    let code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("metadata bad status line: {status_line:?}"))?;
    if code != "200" {
        return Err(format!("metadata HTTP {code} for {METADATA_CURVE_PATH}"));
    }
    Ok(body.to_owned())
}

fn parse_z85_public_key(body: &str) -> Result<String, String> {
    let key = body.trim();
    if key.len() != Z85_KEY_LEN {
        return Err(format!(
            "metadata vsock-pub-key: expected {Z85_KEY_LEN} char Z85 key, got {}",
            key.len()
        ));
    }
    // Z85 alphabet: 0-9 a-z A-Z . - : + = ^ ! / * ? & < > ( ) [ ] { } @ % $ #
    if !key.bytes().all(|b| {
        matches!(b,
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z'
            | b'.' | b'-' | b':' | b'+' | b'=' | b'^' | b'!' | b'/' | b'*' | b'?'
            | b'&' | b'<' | b'>' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'@'
            | b'%' | b'$' | b'#'
        )
    }) {
        return Err("metadata vsock-pub-key: invalid Z85 character".into());
    }
    Ok(key.to_owned())
}
