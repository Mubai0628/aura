fn main() {
    // The windows are drawn at a whole multiple of each monitor's scale, which needs per-monitor
    // DPI awareness, and a declared execution level keeps Windows from treating a program named
    // "Setup" as one that needs administrator rights.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("Aura.Setup"))
            .expect("aura-setup requires an embedded Windows application manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
