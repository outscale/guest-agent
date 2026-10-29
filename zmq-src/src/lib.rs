use std::{env, path::PathBuf};

#[derive(Default)]
pub struct Build {
    src: Option<PathBuf>,
    staticbuild: Option<bool>,
    curve: Option<bool>,
    vsock: Option<bool>,
}

impl Build {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sb(mut self, flag: bool) -> Self {
        self.staticbuild = Some(flag);
        self
    }

    pub fn curve(mut self, flag: bool) -> Self {
        self.curve = Some(flag);
        self
    }

    pub fn vsock(mut self, flag: bool) -> Self {
        self.vsock = Some(flag);
        self
    }

    pub fn build(self) -> PathBuf {
        let src = if let Some(src) = self.src.as_ref() {
            src.clone()
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor")
        };

        let mut config = cmake::Config::new(src);
        config.define("ENABLE_DRAFTS", "OFF");
        config.define("BUILD_TESTS", "OFF");
        config.define("WITH_PERF_TOOL", "OFF");

        let sb = self.staticbuild.unwrap_or(true);
        if sb {
            config.define("BUILD_SHARED", "OFF");
            config.define("BUILD_STATIC", "ON");
        }

        if self.curve.unwrap_or(true) {
            config.define("ENABLE_CURVE", "ON");
            config.define("WITH_LIBSODIUM", "ON");
            if sb {
                config.define("WITH_LIBSODIUM_STATIC", "ON");
                println!("cargo:rustc-link-lib=static=sodium");
            }
        }

        if self.vsock.unwrap_or(true) {
            config.define("ZMQ_HAVE_VSOCK", "ON");
        }

        config.build()
    }
}
