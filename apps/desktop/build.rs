use std::path::Path;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(screenroom_ds_pinned)");
    println!("cargo:rerun-if-changed=build.rs");

    let pinned = Path::new("src/design/ds_pinned.rs");
    println!("cargo:rerun-if-changed=src/design/ds_pinned.rs");
    if pinned.exists() {
        println!("cargo:rustc-cfg=screenroom_ds_pinned");
    }
}
