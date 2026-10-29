use std::sync::Arc;

#[derive(Debug)]
pub struct ZmqError {
    error_code: i32,
}

impl std::fmt::Display for ZmqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SAFETY: zmq_strerror returns a pointer to a static NUL-terminated
        // string valid for the process lifetime, or NULL which is handled.
        let ptr = unsafe { zmq_sys::zmq_strerror(self.error_code) };
        let message = if ptr.is_null() {
            "Unknown error"
        } else {
            // SAFETY: ptr is non-NULL and points at the NUL-terminated C
            // string returned by zmq_strerror (static storage, never freed).
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .unwrap_or("Unknown error")
        };

        write!(f, "ZMQ Error [{}]: {}", self.error_code, message)
    }
}

impl std::error::Error for ZmqError {}

impl ZmqError {
    pub(crate) fn from_errno() -> Self {
        // SAFETY: zmq_errno is a plain thread-local getter, no preconditions.
        let error_code = unsafe { zmq_sys::zmq_errno() };
        ZmqError { error_code }
    }

    pub fn code(&self) -> i32 {
        self.error_code
    }

    pub fn is_eagain(&self) -> bool {
        std::io::Error::from_raw_os_error(self.error_code).kind() == std::io::ErrorKind::WouldBlock
    }
}

struct ZmqRawContext {
    ptr: *mut std::os::raw::c_void,
}

impl ZmqRawContext {
    pub fn new() -> Result<Self, ZmqError> {
        // SAFETY: zmq_ctx_new takes no arguments and returns either a valid
        // context or NULL, which is checked below.
        let ptr = unsafe { zmq_sys::zmq_ctx_new() };
        if ptr.is_null() {
            Err(ZmqError::from_errno())
        } else {
            Ok(ZmqRawContext { ptr })
        }
    }
}

impl Drop for ZmqRawContext {
    fn drop(&mut self) {
        // SAFETY: ptr is the live context created by zmq_ctx_new; this Drop is
        // its only owner (Arc) so no socket outlives it (_ctx keeps it alive).
        unsafe {
            zmq_sys::zmq_ctx_term(self.ptr);
        }
    }
}

// SAFETY: libzmq contexts are thread-safe — zmq_ctx_* may be called
// concurrently from any thread, and the raw pointer is never mutated by hand.
unsafe impl Sync for ZmqRawContext {}
// SAFETY: libzmq contexts are thread-safe; moving the owner between threads is
// sound and termination happens exactly once on the owning thread.
unsafe impl Send for ZmqRawContext {}

#[derive(Clone)]
pub struct ZmqContext {
    inner: Arc<ZmqRawContext>,
}

impl ZmqContext {
    pub fn new() -> Result<Self, ZmqError> {
        let inner = ZmqRawContext::new()?;
        Ok(ZmqContext {
            inner: Arc::new(inner),
        })
    }

    pub fn socket(&self, socket_type: i32) -> Result<ZmqSocket, ZmqError> {
        ZmqSocket::new(self, socket_type)
    }
}

pub const ZMQ_REQ: i32 = 3;
pub const ZMQ_REP: i32 = 4;
pub const ZMQ_DEALER: i32 = 5;
pub const ZMQ_ROUTER: i32 = 6;

pub const ZMQ_ROUTING_ID: i32 = 5;
pub const ZMQ_IDENTITY: i32 = ZMQ_ROUTING_ID;
pub const ZMQ_RCVMORE: i32 = 13;
pub const ZMQ_FD: i32 = 14;
pub const ZMQ_EVENTS: i32 = 15;
pub const ZMQ_LINGER: i32 = 17;
pub const ZMQ_SNDHWM: i32 = 23;
pub const ZMQ_RCVTIMEO: i32 = 27;
pub const ZMQ_SNDTIMEO: i32 = 28;
pub const ZMQ_IMMEDIATE: i32 = 39;

pub const ZMQ_CURVE_SERVER: i32 = 47;
pub const ZMQ_CURVE_PUBLICKEY: i32 = 48;
pub const ZMQ_CURVE_SECRETKEY: i32 = 49;
pub const ZMQ_CURVE_SERVERKEY: i32 = 50;
pub const ZMQ_ZAP_DOMAIN: i32 = 55;

pub const ZMQ_DONTWAIT: i32 = 1;
pub const ZMQ_SNDMORE: i32 = 2;

pub const ZMQ_POLLIN: i16 = 1;
pub const ZMQ_POLLOUT: i16 = 2;

/// ZmqSocket is not thread-safe.
pub struct ZmqSocket {
    ptr: *mut std::os::raw::c_void,
    _ctx: ZmqContext,
}

impl ZmqSocket {
    fn new(context: &ZmqContext, socket_type: i32) -> Result<Self, ZmqError> {
        // SAFETY: context.inner.ptr is a live context (kept alive by the Arc
        // we clone below) and socket_type is a valid ZMQ socket type.
        let ptr = unsafe { zmq_sys::zmq_socket(context.inner.ptr, socket_type) };
        if ptr.is_null() {
            Err(ZmqError::from_errno())
        } else {
            Ok(Self {
                ptr,
                _ctx: context.clone(),
            })
        }
    }

    pub fn bind(&self, address: &str) -> Result<(), ZmqError> {
        let c_address =
            std::ffi::CString::new(address).map_err(|_nul| ZmqError { error_code: -1 })?;
        // SAFETY: self.ptr is a live socket and c_address is a valid
        // NUL-terminated string (no interior NUL by construction).
        let result = unsafe { zmq_sys::zmq_bind(self.ptr, c_address.as_ptr()) };
        if result == 0 {
            Ok(())
        } else {
            Err(ZmqError::from_errno())
        }
    }

    pub fn connect(&self, address: &str) -> Result<(), ZmqError> {
        let c_address =
            std::ffi::CString::new(address).map_err(|_nul| ZmqError { error_code: -1 })?;
        // SAFETY: self.ptr is a live socket and c_address is a valid
        // NUL-terminated string (no interior NUL by construction).
        let result = unsafe { zmq_sys::zmq_connect(self.ptr, c_address.as_ptr()) };
        if result == 0 {
            Ok(())
        } else {
            Err(ZmqError::from_errno())
        }
    }

    pub fn setsockopt(&self, option: i32, value: &[u8]) -> Result<(), ZmqError> {
        // SAFETY: self.ptr is a live socket; value.as_ptr() points at
        // value.len() readable bytes which libzmq copies during the call.
        let result = unsafe {
            zmq_sys::zmq_setsockopt(
                self.ptr,
                option,
                value.as_ptr() as *const std::os::raw::c_void,
                value.len(),
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(ZmqError::from_errno())
        }
    }

    pub fn setsockopt_int(&self, option: i32, value: i32) -> Result<(), ZmqError> {
        self.setsockopt(option, &value.to_ne_bytes())
    }

    pub fn getsockopt_int(&self, option: i32) -> Result<i32, ZmqError> {
        let mut value: i32 = 0;
        let mut size = std::mem::size_of::<i32>();
        // SAFETY: self.ptr is a live socket; &mut value and &mut size are valid
        // and size holds the optvallen of the i32 options used here.
        let result = unsafe {
            zmq_sys::zmq_getsockopt(
                self.ptr,
                option,
                &mut value as *mut i32 as *mut std::os::raw::c_void,
                &mut size,
            )
        };
        if result == 0 {
            Ok(value)
        } else {
            Err(ZmqError::from_errno())
        }
    }

    pub fn set_identity(&self, identity: &[u8]) -> Result<(), ZmqError> {
        self.setsockopt(ZMQ_IDENTITY, identity)
    }

    pub fn set_linger(&self, ms: i32) -> Result<(), ZmqError> {
        self.setsockopt_int(ZMQ_LINGER, ms)
    }

    pub fn set_sndtimeo(&self, ms: i32) -> Result<(), ZmqError> {
        self.setsockopt_int(ZMQ_SNDTIMEO, ms)
    }

    pub fn set_rcvtimeo(&self, ms: i32) -> Result<(), ZmqError> {
        self.setsockopt_int(ZMQ_RCVTIMEO, ms)
    }

    pub fn set_sndhwm(&self, hwm: i32) -> Result<(), ZmqError> {
        self.setsockopt_int(ZMQ_SNDHWM, hwm)
    }

    pub fn set_immediate(&self, enabled: bool) -> Result<(), ZmqError> {
        self.setsockopt_int(ZMQ_IMMEDIATE, i32::from(enabled))
    }

    pub fn set_curve_server(&self, secret_key: &str, public_key: &str) -> Result<(), ZmqError> {
        self.setsockopt(ZMQ_CURVE_SECRETKEY, secret_key.as_bytes())?;
        self.setsockopt(ZMQ_CURVE_PUBLICKEY, public_key.as_bytes())?;
        self.setsockopt_int(ZMQ_CURVE_SERVER, 1)
    }

    pub fn set_zap_domain(&self, domain: &str) -> Result<(), ZmqError> {
        self.setsockopt(ZMQ_ZAP_DOMAIN, domain.as_bytes())
    }

    pub fn recv(&self, flags: i32) -> Result<Vec<u8>, ZmqError> {
        let mut msg = std::mem::MaybeUninit::<zmq_sys::zmq_msg_t>::uninit();
        // SAFETY: msg is an uninitialized zmq_msg_t slot, exactly what
        // zmq_msg_init requires.
        if unsafe { zmq_sys::zmq_msg_init(msg.as_mut_ptr()) } != 0 {
            return Err(ZmqError::from_errno());
        }
        // SAFETY: zmq_msg_init succeeded above, so the slot now owns a valid
        // initialized message; the reference borrows `msg` for its lifetime.
        let msg = unsafe { &mut *msg.as_mut_ptr() };

        // SAFETY: msg is a valid initialized message and self.ptr a live
        // socket; libzmq fills the message on success.
        let result = unsafe { zmq_sys::zmq_msg_recv(msg, self.ptr, flags) };
        if result < 0 {
            let err = ZmqError::from_errno();
            // SAFETY: msg is initialized and not yet closed.
            unsafe { zmq_sys::zmq_msg_close(msg) };
            return Err(err);
        }

        // SAFETY: msg is initialized and received; zmq_msg_data returns a
        // pointer to its payload, valid until zmq_msg_close below.
        let data = unsafe { zmq_sys::zmq_msg_data(msg) as *const u8 };
        // SAFETY: msg is the same valid received message as above.
        let size = unsafe { zmq_sys::zmq_msg_size(msg) };
        // SAFETY: data points at size payload bytes owned by msg; this copies
        // them out before the message is closed.
        let buffer = unsafe { std::slice::from_raw_parts(data, size).to_vec() };
        // SAFETY: msg is initialized and not yet closed; this frees it.
        unsafe { zmq_sys::zmq_msg_close(msg) };
        Ok(buffer)
    }

    pub fn send(&self, data: &[u8], flags: i32) -> Result<(), ZmqError> {
        // SAFETY: self.ptr is a live socket; data.as_ptr() points at
        // data.len() readable bytes which libzmq copies during the call.
        let result = unsafe {
            zmq_sys::zmq_send(
                self.ptr,
                data.as_ptr() as *const std::os::raw::c_void,
                data.len(),
                flags,
            )
        };
        if result >= 0 {
            Ok(())
        } else {
            Err(ZmqError::from_errno())
        }
    }

    pub fn recv_multipart(&self, flags: i32) -> Result<Vec<Vec<u8>>, ZmqError> {
        let mut frames = Vec::new();
        loop {
            frames.push(self.recv(flags)?);
            if self.getsockopt_int(ZMQ_RCVMORE)? == 0 {
                break;
            }
        }
        Ok(frames)
    }

    pub fn send_multipart(&self, frames: &[impl AsRef<[u8]>], flags: i32) -> Result<(), ZmqError> {
        if frames.is_empty() {
            return self.send(&[], flags);
        }
        let last = frames.len() - 1;
        for (i, frame) in frames.iter().enumerate() {
            let f = if i == last {
                flags
            } else {
                flags | ZMQ_SNDMORE
            };
            self.send(frame.as_ref(), f)?;
        }
        Ok(())
    }

    /// Poll this socket. `timeout_ms` -1 blocks, 0 returns immediately.
    /// Returns revents (0 on timeout).
    pub fn poll(&self, events: i16, timeout_ms: i64) -> Result<i16, ZmqError> {
        let mut item = zmq_sys::zmq_pollitem_t {
            socket: self.ptr,
            fd: -1,
            events,
            revents: 0,
        };
        // SAFETY: item is a valid zmq_pollitem_t holding a live socket and
        // nitems is 1, matching the single element behind &mut item.
        let rc = unsafe { zmq_sys::zmq_poll(&mut item, 1, timeout_ms as std::os::raw::c_long) };
        if rc < 0 {
            Err(ZmqError::from_errno())
        } else {
            Ok(item.revents)
        }
    }
}

impl Drop for ZmqSocket {
    fn drop(&mut self) {
        // SAFETY: ptr is a live socket owned solely by this wrapper; ZmqSocket
        // is !Sync so no other thread can use it concurrently.
        unsafe {
            zmq_sys::zmq_close(self.ptr);
        }
    }
}

// SAFETY: a ZmqSocket is owned by one thread at a time and only moved between
// threads (!Sync prevents concurrent use, which libzmq forbids).
unsafe impl Send for ZmqSocket {}

fn z85_key_string(buf: &[u8]) -> String {
    // Z85 output buffers are NUL-terminated; keep the prefix before the first NUL.
    let key = buf.split(|&b| b == 0).next().unwrap_or(buf);
    String::from_utf8_lossy(key).into_owned()
}

pub fn zmq_curve_keypair() -> Result<(String, String), ZmqError> {
    let mut public_key = [0u8; 41];
    let mut secret_key = [0u8; 41];
    // SAFETY: both buffers are 41 bytes (40 Z85 chars + NUL), the size
    // zmq_curve_keypair requires, and writable.
    let result = unsafe {
        zmq_sys::zmq_curve_keypair(
            public_key.as_mut_ptr() as *mut i8,
            secret_key.as_mut_ptr() as *mut i8,
        )
    };

    if result == 0 {
        Ok((z85_key_string(&public_key), z85_key_string(&secret_key)))
    } else {
        Err(ZmqError::from_errno())
    }
}

pub fn z85_encode(data: &[u8]) -> Result<String, ZmqError> {
    if data.is_empty() || !data.len().is_multiple_of(4) {
        return Err(ZmqError { error_code: -1 });
    }
    let mut buf = vec![0u8; data.len() / 4 * 5 + 1];
    // SAFETY: buf holds 5 output chars per 4 input bytes plus the NUL
    // terminator; data is data.len() readable bytes (a multiple of 4).
    let rc =
        unsafe { zmq_sys::zmq_z85_encode(buf.as_mut_ptr() as *mut i8, data.as_ptr(), data.len()) };
    if rc.is_null() {
        return Err(ZmqError::from_errno());
    }
    Ok(z85_key_string(&buf))
}
