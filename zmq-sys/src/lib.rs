// bindgen output mirrors C naming conventions from zmq.h.
#![expect(non_camel_case_types, reason = "bindgen names mirror C")]
// bindgen's layout tests use the `["x"][idx]` const-assert idiom plus its own
// `#[allow(...)]` attribute on the generated const blocks.
#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::indexing_slicing,
    reason = "bindgen-generated layout tests"
)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
