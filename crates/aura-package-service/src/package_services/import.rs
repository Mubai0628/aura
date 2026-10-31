//! Import from a path Main obtained from its own dialog. Staging copies and checks
//! the source; the V2 transaction keeps content only. Nothing is installed or run.
use super::PackageServices;
use anyhow::Result;
use aura_tool_domain::{PackageImportOutcomeV1, PackageVersionRefV1};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportSource {
    Directory,
    Archive,
}

impl PackageServices {
    pub fn import(&self, source: ImportSource, path: &Path) -> Result<PackageImportOutcomeV1> {
        anyhow::ensure!(path.is_absolute(), "package.import_path_absolute");
        let staged = match source {
            ImportSource::Directory => {
                aura_tool_package::stage_directory_package_v1(path, &self.import_root)
            }
            ImportSource::Archive => {
                aura_tool_package::stage_zip_package_v1(path, &self.import_root)
            }
        };
        let staged = match staged {
            Ok(staged) => staged,
            Err(error) => {
                return Ok(PackageImportOutcomeV1::Rejected {
                    diagnostics: vec![aura_tool_package::package_import_diagnostic_v1(&error)],
                });
            }
        };
        let binding = self.owner.import_staged(&staged)?;
        drop(staged);
        let view = self.version_view(&PackageVersionRefV1 {
            package_id: binding.package_id,
            package_version: binding.package_version,
            package_fingerprint: binding.package_fingerprint,
        })?;
        Ok(PackageImportOutcomeV1::Imported { view: Box::new(view) })
    }
}
