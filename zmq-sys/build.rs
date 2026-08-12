use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PROFILE");

    let dst = zmq_src::Build::new().build();

    let mut pc_path = Vec::new();
    for sub in ["lib64/pkgconfig", "lib/pkgconfig"] {
        let p = dst.join(sub);
        if p.is_dir() {
            pc_path.push(p);
        }
    }

    if let Ok(old) = env::var("PKG_CONFIG_PATH") {
        pc_path.extend(env::split_paths(&old));
    }

    // SAFETY: the build script is single-threaded at this point (nothing has
    // been spawned in this process), so mutating the environment cannot race
    // with a concurrent env read.
    unsafe {
        env::set_var(
            "PKG_CONFIG_PATH",
            env::join_paths(&pc_path).expect("PKG_CONFIG_PATH entries contain no path separator"),
        );
    }

    let lib = pkg_config::Config::new()
        .statik(true)
        .cargo_metadata(true)
        .probe("libzmq")
        .expect("libzmq.pc from zmq-src install");

    // Gen Binding
    let header = lib
        .include_paths
        .iter()
        .map(|p| p.join("zmq.h"))
        .find(|p| p.is_file())
        .expect("zmq.h from zmq-src install");
    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_function("zmq_.*")
        .generate()
        .expect("Unable to generate binding");

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
