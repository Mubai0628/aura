//! Test-only: a real package install through Core's own package service, for
//! scenarios outside this module (Agent registry and execution).
use crate::package_services::{ImportSource, PackageServices, RunEnvironment};
use aura_package_service::package_program::tests::support::{Fixture as Source, Kind};
use aura_tool_domain::*;
use std::time::{Duration, Instant};

/// The installed package and the temporary source that must outlive the test.
pub(crate) struct Installed {
    pub _source: Source,
    pub summary: PackageSummaryV1,
}
impl Installed {
    pub fn tool_name(&self) -> String {
        self.summary.installed.as_ref().expect("installed").tools[0]
            .canonical_name
            .as_str()
            .to_owned()
    }
}

pub(crate) fn packages(service: &crate::RuntimeServices) -> &PackageServices {
    service.packages.as_deref().expect("Core opened its package service")
}

pub(crate) fn install(
    service: &crate::RuntimeServices,
    kind: Kind,
    edit: impl FnOnce(&mut serde_json::Value),
) -> Installed {
    let source = Source::new(kind, edit);
    let packages = packages(service);
    let PackageImportOutcomeV1::Imported { view } = packages
        .import(ImportSource::Directory, &source.root.path().join("source"))
        .expect("import")
    else {
        panic!("the fixture package imports")
    };
    let prepared = packages
        .prepare(
            &PackageConformancePrepareRequestV1 {
                version: view.preview.version.clone(),
                settings: vec![],
            },
            RunEnvironment {
                powershell: None,
                user_environment: source.context().user_environment,
            },
        )
        .expect("concrete preview");
    packages
        .start(&PackageConformanceStartRequestV1 {
            preparation_id: prepared.preparation_id,
            preview_hash: prepared.preview_hash,
        })
        .expect("approved Conformance start");
    let reference = PackageVersionRefV1 {
        package_id: view.preview.version.package_id,
        package_version: view.preview.version.package_version.clone(),
        package_fingerprint: view.preview.version.package_fingerprint.clone(),
    };
    let until = Instant::now() + Duration::from_secs(60);
    let run = loop {
        let current = packages.conformance(&reference).expect("read run");
        if current.state != PackageConformanceStateV1::Running {
            break current;
        }
        assert!(Instant::now() < until, "Conformance must settle");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(run.state, PackageConformanceStateV1::Succeeded, "{:?}", run.report);
    let revision = packages.summary(reference.package_id).expect("listed").revision;
    let summary = packages
        .install(&PackageInstallRequestV1 {
            version: run.version.clone(),
            expected_package_revision: revision,
            report_hash: run.report_hash.clone().expect("finished run hash"),
        })
        .expect("explicit install");
    Installed { _source: source, summary }
}

pub(crate) fn set_enabled(
    service: &crate::RuntimeServices,
    package: aura_domain_common::ToolPackageId,
    enabled: bool,
) -> PackageSummaryV1 {
    let packages = packages(service);
    let revision = packages.summary(package).expect("listed").revision;
    packages
        .set_enabled(&PackageEnableRequestV1 {
            package_id: package,
            expected_package_revision: revision,
            enabled,
        })
        .expect("lifecycle change")
}

pub(crate) fn uninstall(
    service: &crate::RuntimeServices,
    package: aura_domain_common::ToolPackageId,
) -> PackageSummaryV1 {
    let packages = packages(service);
    let revision = packages.summary(package).expect("listed").revision;
    packages
        .uninstall(&PackageChangeRequestV1 {
            package_id: package,
            expected_package_revision: revision,
        })
        .expect("uninstall")
}

/// How many package payloads the V2 store still holds.
pub(crate) fn payloads(service: &crate::RuntimeServices) -> i64 {
    service
        .broker
        .lock()
        .expect("V2 store")
        .database
        .connection()
        .query_row("SELECT count(*) FROM tool_package_payloads", [], |row| row.get(0))
        .expect("payload count")
}
