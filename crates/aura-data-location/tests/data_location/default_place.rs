//! Without a location record every location is where Aura has always kept it.
use crate::support;

use aura_data_location::{InstallCopy, Locations};
use support::{Profile, production, test_namespace};

#[test]
fn the_released_product_without_a_record_keeps_todays_layout() {
    let profile = Profile::new();
    let roots = &profile.roots;
    let namespace = production();
    let hash = namespace.object_name_hash();
    let locations =
        Locations::resolve(roots, &namespace, &InstallCopy::Installed(None)).expect("locations");
    let fixed = roots.local.join("Aura/namespaces").join(&hash);
    assert_eq!(locations.data_root, fixed);
    assert_eq!(locations.fixed_root, fixed);
    assert_eq!(locations.settings_root, roots.roaming.join("Aura/namespaces").join(&hash));
    assert_eq!(locations.core_log, roots.local.join("Aura/logs/core.jsonl"));
    assert_eq!(locations.ui_profile, roots.roaming.join("@aura/desktop"));
    assert_eq!(locations.chrome_root, fixed.join("chrome"));
    assert_eq!(locations.record, fixed.join("location.v1.json"));
    assert_eq!(locations.location, None);
    assert!(locations.pending_removal.is_empty());
    assert_eq!(locations.aura_roots, [roots.roaming.join("Aura"), roots.local.join("Aura")]);
}

#[test]
fn another_namespace_has_an_electron_profile_of_its_own() {
    let profile = Profile::new();
    let namespace = test_namespace();
    let locations = Locations::resolve(&profile.roots, &namespace, &InstallCopy::NotInstalled)
        .expect("locations");
    let released = Locations::resolve(&profile.roots, &production(), &InstallCopy::NotInstalled)
        .expect("released");
    assert_eq!(locations.ui_profile, locations.data_root.join("ui-profile"));
    assert_ne!(locations.ui_profile, released.ui_profile);
    // Core's own log stays where every namespace has written it.
    assert_eq!(locations.core_log, released.core_log);
}

#[test]
fn working_the_locations_out_creates_nothing() {
    let profile = Profile::new();
    let before = profile.listing();
    Locations::resolve(&profile.roots, &production(), &InstallCopy::Installed(None))
        .expect("locations")
        .with_global_memory(&profile.roots, &production())
        .expect("global memory");
    assert_eq!(profile.listing(), before);
}
