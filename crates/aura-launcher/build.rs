fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("Aura.Launcher"))
            .expect("AuraLauncher requires an embedded Windows application manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
