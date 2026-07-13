use std::env;
use std::fs;
use std::path::Path;

use embed_manifest::{embed_manifest, new_manifest};

/// Embed a Windows application manifest with `requestedExecutionLevel = asInvoker`.
///
/// The crate is named `omni_setup`, so every executable Cargo produces from it
/// (including the unit-test binary `omni_setup-<hash>.exe`) has a filename that
/// matches Windows' UAC "installer detection" heuristic. Without a manifest,
/// Windows assumes such executables are installers and refuses to launch them
/// without elevation, which makes `cargo test` fail with:
///
///     The requested operation requires elevation. (os error 740)
///
/// `embed_manifest` only emits `cargo:rustc-link-arg-bins=...`, so it fixes real
/// binaries but NOT test/bench/example executables (and errors on a lib-only
/// crate that has no bin target). We therefore emit the unscoped
/// `cargo:rustc-link-arg`, which applies to every linked artifact this crate
/// produces (tests, benches, examples, bins) on MSVC targets.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let target = env::var("TARGET").unwrap_or_default();
    let manifest = new_manifest("Omni.Setup");

    if target.ends_with("windows-msvc") {
        let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
        let manifest_path = Path::new(&out_dir).join("omni_setup.manifest");
        fs::write(&manifest_path, manifest.to_string())
            .expect("unable to write manifest file");
        let manifest_path = manifest_path
            .canonicalize()
            .expect("unable to canonicalize manifest path");

        // Unscoped `rustc-link-arg` applies to bins, tests, benches and
        // examples, so the `cargo test` executable gets the manifest too.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest_path.display()
        );
        println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
    } else {
        // GNU/LLVM targets: embed into binaries via the crate's own COFF-resource
        // path. Installer detection is a 32-bit-only heuristic there.
        embed_manifest(manifest).expect("unable to embed manifest file");
    }
}
