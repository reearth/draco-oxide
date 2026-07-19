//! Compiles the libdraco shim when the Google Draco checkout built by
//! `scripts/build-draco.sh` is present (override with `DRACO_SRC_DIR` /
//! `DRACO_BUILD_DIR`). Without it the crate still builds; the binary then
//! explains how to get the library at runtime.

use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(have_libdraco)");
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-env-changed=DRACO_SRC_DIR");
    println!("cargo:rerun-if-env-changed=DRACO_BUILD_DIR");

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let src = std::env::var("DRACO_SRC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("third_party/draco"));
    let build = std::env::var("DRACO_BUILD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| src.join("_build"));

    let lib = build.join("libdraco.a");
    let header = src.join("src/draco/compression/encode.h");
    if !lib.is_file() || !header.is_file() {
        println!(
            "cargo:warning=libdraco not found ({} / {}); `bench` will ask for \
             scripts/build-draco.sh at runtime",
            lib.display(),
            header.display()
        );
        return;
    }

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include(src.join("src"))
        .include(&build)
        .file("src/shim.cc")
        .warnings(false)
        .compile("draco_bench_shim");

    println!("cargo:rustc-link-search=native={}", build.display());
    // whole-archive keeps draco's static-initializer-registered file readers,
    // which the linker would otherwise drop from the archive.
    println!("cargo:rustc-link-lib=static:+whole-archive=draco");
    println!("cargo:rustc-cfg=have_libdraco");
}
