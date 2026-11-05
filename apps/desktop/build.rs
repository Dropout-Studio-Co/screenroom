use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(screenroom_ds_pinned)");
    println!("cargo:rustc-check-cfg=cfg(screenroom_updater)");
    println!("cargo:rerun-if-changed=build.rs");

    let pinned = Path::new("src/design/ds_pinned.rs");
    println!("cargo:rerun-if-changed=src/design/ds_pinned.rs");
    if pinned.exists() {
        println!("cargo:rustc-cfg=screenroom_ds_pinned");
    }

    link_sparkle();
}

fn link_sparkle() {
    println!("cargo:rerun-if-changed=native/screenroom_updater.m");
    println!("cargo:rerun-if-changed=native/screenroom_updater.h");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let vendor = manifest.join("../../vendor/sparkle");
    println!("cargo:rerun-if-changed={}", vendor.display());
    println!("cargo:rerun-if-env-changed=SCREENROOM_SPARKLE_DIR");

    let sparkle_root = match std::env::var_os("SCREENROOM_SPARKLE_DIR") {
        Some(directory) => PathBuf::from(directory),
        None => vendor,
    };
    if !sparkle_root.join("Sparkle.framework").is_dir() {
        println!(
            "cargo:warning=Sparkle.framework is absent from {}; the build reports updates as unavailable. Run scripts/fetch-sparkle.sh to enable them.",
            sparkle_root.display()
        );
        return;
    }

    let source = manifest.join("native/screenroom_updater.m");
    let object = PathBuf::from(std::env::var("OUT_DIR").expect("out directory"))
        .join("screenroom_updater_bridge.o");
    let compiled = Command::new("clang")
        .arg("-c")
        .arg("-fobjc-arc")
        .arg("-fmodules")
        .arg("-mmacosx-version-min=12.0")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-Wno-deprecated-declarations")
        .arg(format!("-F{}", sparkle_root.display()))
        .arg("-I")
        .arg(manifest.join("native"))
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .status()
        .expect("clang is part of the macOS command line tools");
    assert!(compiled.success(), "the update bridge did not compile");
    println!("cargo:rustc-link-arg={}", object.display());

    println!(
        "cargo:rustc-link-search=framework={}",
        sparkle_root.display()
    );
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,Sparkle");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rustc-cfg=screenroom_updater");
}
