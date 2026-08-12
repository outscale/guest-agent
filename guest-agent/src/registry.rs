use serde_json::Value;
use std::{collections::BTreeMap, sync::LazyLock};

use log::{debug, info, warn};

use crate::rpc_models::{self, RpcError};

pub trait JsonRpcMethod: Send + Sync + 'static {
    const NAME: &'static str;

    fn call(arguments: Value) -> rpc_models::Result<Value>;
}

pub struct JsonRpcMethodRegistration {
    pub name: &'static str,
    pub call: fn(Value) -> rpc_models::Result<Value>,
}

inventory::collect!(JsonRpcMethodRegistration);

pub struct JsonRpcMethodRegistry {
    methods: BTreeMap<&'static str, &'static JsonRpcMethodRegistration>,
}

impl JsonRpcMethodRegistry {
    fn build() -> Self {
        let mut methods = BTreeMap::new();

        for registration in inventory::iter::<JsonRpcMethodRegistration> {
            #[expect(
                clippy::panic,
                reason = "duplicate method name is a code bug in the inventory registrations; fail fast at startup"
            )]
            if methods.insert(registration.name, registration).is_some() {
                panic!("duplicate method registration: {}", registration.name);
            }
            debug!(
                target: "guest_agent::rpc",
                "registered method {}",
                registration.name
            );
        }

        info!(
            target: "guest_agent::rpc",
            "JSON-RPC registry ready ({} method{})",
            methods.len(),
            if methods.len() == 1 { "" } else { "s" }
        );
        if log::log_enabled!(target: "guest_agent::rpc", log::Level::Debug) {
            for name in methods.keys() {
                debug!(target: "guest_agent::rpc", " - {name}");
            }
        }

        Self { methods }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: LazyLock<JsonRpcMethodRegistry> =
            LazyLock::new(JsonRpcMethodRegistry::build);
        &REGISTRY
    }

    fn get(&self, name: &str) -> Option<&'static JsonRpcMethodRegistration> {
        self.methods.get(name).copied()
    }

    /// Dispatch a request. Returns `None` for notifications (no `id`).
    pub fn dispatch(&self, req: &rpc_models::Request) -> Option<rpc_models::Response> {
        let id = req.id.clone();
        let id_log = id
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into());

        debug!(
            target: "guest_agent::rpc",
            "dispatch method={} id={id_log}",
            req.method
        );

        let Some(reg) = self.get(&req.method) else {
            warn!(
                target: "guest_agent::rpc",
                "method not found: {} id={id_log}",
                req.method
            );
            return id.map(|id| rpc_models::Response::error(id, RpcError::method_not_found()));
        };

        let params = req.params.clone().unwrap_or_else(|| serde_json::json!({}));
        debug!(
            target: "guest_agent::rpc",
            "calling {} id={id_log} params={}",
            reg.name,
            params
        );

        let outcome = (reg.call)(params);
        match &outcome {
            Ok(result) => {
                debug!(
                    target: "guest_agent::rpc",
                    "method {} ok id={id_log} result={}",
                    reg.name,
                    result
                );
            }
            Err(err) => {
                warn!(
                    target: "guest_agent::rpc",
                    "method {} error id={id_log} code={} message={}",
                    reg.name,
                    err.code,
                    err.message
                );
            }
        }

        id.map(|id| match outcome {
            Ok(result) => rpc_models::Response::result(id, Box::new(result)),
            Err(err) => rpc_models::Response::error(id, err),
        })
    }
}
