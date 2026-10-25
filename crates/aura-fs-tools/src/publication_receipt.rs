//! Actual handle-bound publication receipts. Scoped to one synchronous file invocation.
use aura_domain_common::FileIdentity;
use aura_windows_platform::{NativeFileIdentity, NativePathInfo, VerifiedFile, VolumeCaseRule};
use std::cell::RefCell;

#[derive(Clone, Debug)]
pub struct FilePublicationReceipt {
    pub entry_key: String,
    pub previous_identity: Option<FileIdentity>,
    pub published_identity: FileIdentity,
}

thread_local! {
    static RECEIPTS: RefCell<Option<Vec<FilePublicationReceipt>>> = const { RefCell::new(None) };
}

/// The collector observes a real publication; it cannot grant authority or alter a write.
pub fn with_file_publications<T>(work: impl FnOnce() -> T) -> (T, Vec<FilePublicationReceipt>) {
    struct Restore(Option<Vec<FilePublicationReceipt>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            RECEIPTS.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(RECEIPTS.with(|slot| slot.replace(Some(Vec::new()))));
    let result = work();
    let receipts = RECEIPTS.with(|slot| slot.borrow_mut().take().unwrap_or_default());
    (result, receipts)
}

pub(crate) fn published(
    parent: &VerifiedFile,
    leaf: &std::ffi::OsStr,
    previous: Option<&NativeFileIdentity>,
    info: &NativePathInfo,
) {
    RECEIPTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(receipts) = slot.as_mut() else { return };
        let name = leaf.to_string_lossy();
        let key = match aura_windows_platform::directory_case_rule(parent) {
            VolumeCaseRule::Sensitive => name.into_owned(),
            VolumeCaseRule::InsensitivePreserving => {
                let Some(map) = aura_windows_platform::directory_name_comparison(parent) else {
                    return;
                };
                map.key(&name)
            }
            VolumeCaseRule::Unknown => return,
        };
        let parent_id = aura_workspace::domain_identity(&parent.info().identity);
        receipts.push(FilePublicationReceipt {
            entry_key: format!(
                "entry:{}:{}:{}",
                parent_id.volume_serial_number, parent_id.file_id, key
            ),
            previous_identity: previous.map(aura_workspace::domain_identity),
            published_identity: aura_workspace::domain_identity(&info.identity),
        });
    });
}
