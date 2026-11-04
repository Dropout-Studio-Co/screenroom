use std::path::{Path, PathBuf};

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

    let mut build = cc::Build::new();
    build
        .file(manifest.join("native/screenroom_updater.m"))
        .include(manifest.join("native"))
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .flag(format!("-F{}", sparkle_root.display()))
        .flag("-mmacosx-version-min=12.0")
        .flag("-Wno-deprecated-declarations")
        .warnings(true);
    build.compile("screenroom_updater_bridge");

    println!(
        "cargo:rustc-link-search=framework={}",
        sparkle_root.display()
    );
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,Sparkle");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rustc-cfg=screenroom_updater");
}
