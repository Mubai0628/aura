//! Moves one file or folder to the Recycle Bin, or leaves it where it
//! is. Windows' own file operation recycles; where it would delete for good instead (a drive with
//! no Recycle Bin, an item larger than the bin holds, the bin switched off), its progress callback
//! sees that before anything happens and Aura cancels the item. Nothing is ever deleted for good
//! here.
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{E_ABORT, E_NOINTERFACE, S_OK};
use windows_sys::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows_sys::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NO_UI, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
    SHCreateItemFromParsingName, TSF_DELETE_RECYCLE_IF_POSSIBLE,
};
use windows_sys::core::{GUID, HRESULT};

/// What became of the item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecycleOutcome {
    /// It is in the Recycle Bin, where the user can restore it.
    Recycled,
    /// Windows would have deleted it for good; it was left where it is.
    NotRecyclable,
}

const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const IID_IFILE_OPERATION: GUID = GUID::from_u128(0x947aab5f_0a5c_4c13_b4d6_4bf7836fc9f8);
const IID_IFILE_OPERATION_PROGRESS_SINK: GUID =
    GUID::from_u128(0x04b0f1a7_9490_44bc_96e1_4296a31252e2);
const IID_ISHELL_ITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);

type Raw = *mut c_void;
type Unused = unsafe extern "system" fn(Raw) -> HRESULT;

/// IFileOperation's table, in ShObjIdl's order; only the entries Aura calls are typed.
#[repr(C)]
struct FileOperationVtbl {
    query_interface: unsafe extern "system" fn(Raw, *const GUID, *mut Raw) -> HRESULT,
    add_ref: unsafe extern "system" fn(Raw) -> u32,
    release: unsafe extern "system" fn(Raw) -> u32,
    advise: unsafe extern "system" fn(Raw, Raw, *mut u32) -> HRESULT,
    unadvise: unsafe extern "system" fn(Raw, u32) -> HRESULT,
    set_operation_flags: unsafe extern "system" fn(Raw, u32) -> HRESULT,
    _set_progress_message: Unused,
    _set_progress_dialog: Unused,
    _set_properties: Unused,
    _set_owner_window: Unused,
    _apply_properties_to_item: Unused,
    _apply_properties_to_items: Unused,
    _rename_item: Unused,
    _rename_items: Unused,
    _move_item: Unused,
    _move_items: Unused,
    _copy_item: Unused,
    _copy_items: Unused,
    delete_item: unsafe extern "system" fn(Raw, Raw, Raw) -> HRESULT,
    _delete_items: Unused,
    _new_item: Unused,
    perform_operations: unsafe extern "system" fn(Raw) -> HRESULT,
    get_any_operations_aborted: unsafe extern "system" fn(Raw, *mut i32) -> HRESULT,
}

#[repr(C)]
struct UnknownVtbl {
    _query_interface: Unused,
    _add_ref: Unused,
    release: unsafe extern "system" fn(Raw) -> u32,
}

/// IFileOperationProgressSink, implemented here: every notification is accepted except a delete
/// that would not recycle.
#[repr(C)]
struct SinkVtbl {
    query_interface: unsafe extern "system" fn(Raw, *const GUID, *mut Raw) -> HRESULT,
    add_ref: unsafe extern "system" fn(Raw) -> u32,
    release: unsafe extern "system" fn(Raw) -> u32,
    start_operations: unsafe extern "system" fn(Raw) -> HRESULT,
    finish_operations: unsafe extern "system" fn(Raw, HRESULT) -> HRESULT,
    pre_rename_item: unsafe extern "system" fn(Raw, u32, Raw, *const u16) -> HRESULT,
    post_rename_item: unsafe extern "system" fn(Raw, u32, Raw, *const u16, HRESULT, Raw) -> HRESULT,
    pre_move_item: unsafe extern "system" fn(Raw, u32, Raw, Raw, *const u16) -> HRESULT,
    post_move_item:
        unsafe extern "system" fn(Raw, u32, Raw, Raw, *const u16, HRESULT, Raw) -> HRESULT,
    pre_copy_item: unsafe extern "system" fn(Raw, u32, Raw, Raw, *const u16) -> HRESULT,
    post_copy_item:
        unsafe extern "system" fn(Raw, u32, Raw, Raw, *const u16, HRESULT, Raw) -> HRESULT,
    pre_delete_item: unsafe extern "system" fn(Raw, u32, Raw) -> HRESULT,
    post_delete_item: unsafe extern "system" fn(Raw, u32, Raw, HRESULT, Raw) -> HRESULT,
    pre_new_item: unsafe extern "system" fn(Raw, u32, Raw, *const u16) -> HRESULT,
    post_new_item: unsafe extern "system" fn(
        Raw,
        u32,
        Raw,
        *const u16,
        *const u16,
        u32,
        HRESULT,
        Raw,
    ) -> HRESULT,
    update_progress: unsafe extern "system" fn(Raw, u32, u32) -> HRESULT,
    reset_timer: unsafe extern "system" fn(Raw) -> HRESULT,
    pause_timer: unsafe extern "system" fn(Raw) -> HRESULT,
    resume_timer: unsafe extern "system" fn(Raw) -> HRESULT,
}

#[repr(C)]
struct Sink {
    vtbl: *const SinkVtbl,
    references: AtomicU32,
    /// Windows was about to delete an item for good; it was cancelled.
    refused: AtomicBool,
    /// An item went to the Recycle Bin.
    recycled: AtomicBool,
}

unsafe extern "system" fn sink_query(this: Raw, iid: *const GUID, out: *mut Raw) -> HRESULT {
    // SAFETY: COM passes a valid interface id and out pointer.
    let iid = unsafe { &*iid };
    let wanted = |known: &GUID| {
        known.data1 == iid.data1
            && known.data2 == iid.data2
            && known.data3 == iid.data3
            && known.data4 == iid.data4
    };
    if wanted(&IID_IUNKNOWN) || wanted(&IID_IFILE_OPERATION_PROGRESS_SINK) {
        // SAFETY: `out` is a valid pointer from COM; the sink outlives the operation.
        unsafe {
            *out = this;
            sink_add_ref(this);
        }
        S_OK
    } else {
        // SAFETY: as above.
        unsafe { *out = std::ptr::null_mut() };
        E_NOINTERFACE
    }
}

unsafe extern "system" fn sink_add_ref(this: Raw) -> u32 {
    // SAFETY: `this` is the sink Aura created.
    unsafe { &*(this as *const Sink) }.references.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn sink_release(this: Raw) -> u32 {
    // SAFETY: `this` is the sink Aura created; the last release frees it.
    let left = unsafe { &*(this as *const Sink) }.references.fetch_sub(1, Ordering::AcqRel) - 1;
    if left == 0 {
        // SAFETY: created with Box::into_raw and nothing refers to it any more.
        drop(unsafe { Box::from_raw(this as *mut Sink) });
    }
    left
}

unsafe extern "system" fn accept(_: Raw) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_finish(_: Raw, _: HRESULT) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_rename(_: Raw, _: u32, _: Raw, _: *const u16) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_post_rename(
    _: Raw,
    _: u32,
    _: Raw,
    _: *const u16,
    _: HRESULT,
    _: Raw,
) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_transfer(
    _: Raw,
    _: u32,
    _: Raw,
    _: Raw,
    _: *const u16,
) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_post_transfer(
    _: Raw,
    _: u32,
    _: Raw,
    _: Raw,
    _: *const u16,
    _: HRESULT,
    _: Raw,
) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_new(_: Raw, _: u32, _: Raw, _: *const u16) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_post_new(
    _: Raw,
    _: u32,
    _: Raw,
    _: *const u16,
    _: *const u16,
    _: u32,
    _: HRESULT,
    _: Raw,
) -> HRESULT {
    S_OK
}
unsafe extern "system" fn accept_progress(_: Raw, _: u32, _: u32) -> HRESULT {
    S_OK
}

/// Before an item is deleted: without "recycle if possible", Windows would delete it for good.
unsafe extern "system" fn pre_delete(this: Raw, flags: u32, _: Raw) -> HRESULT {
    // SAFETY: `this` is the sink Aura created.
    let sink = unsafe { &*(this as *const Sink) };
    if decide(flags) == RecycleOutcome::Recycled {
        S_OK
    } else {
        sink.refused.store(true, Ordering::Release);
        E_ABORT
    }
}

unsafe extern "system" fn post_delete(
    this: Raw,
    _: u32,
    _: Raw,
    result: HRESULT,
    _: Raw,
) -> HRESULT {
    // SAFETY: `this` is the sink Aura created.
    let sink = unsafe { &*(this as *const Sink) };
    if result >= 0 {
        sink.recycled.store(true, Ordering::Release);
    }
    S_OK
}

/// The one decision the callback makes, from the flags Windows gives the item about to go.
fn decide(flags: u32) -> RecycleOutcome {
    if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE as u32 != 0 {
        RecycleOutcome::Recycled
    } else {
        RecycleOutcome::NotRecyclable
    }
}

static SINK_VTBL: SinkVtbl = SinkVtbl {
    query_interface: sink_query,
    add_ref: sink_add_ref,
    release: sink_release,
    start_operations: accept,
    finish_operations: accept_finish,
    pre_rename_item: accept_rename,
    post_rename_item: accept_post_rename,
    pre_move_item: accept_transfer,
    post_move_item: accept_post_transfer,
    pre_copy_item: accept_transfer,
    post_copy_item: accept_post_transfer,
    pre_delete_item: pre_delete,
    post_delete_item: post_delete,
    pre_new_item: accept_new,
    post_new_item: accept_post_new,
    update_progress: accept_progress,
    reset_timer: accept,
    pause_timer: accept,
    resume_timer: accept,
};

fn failed(step: &str, result: HRESULT) -> std::io::Error {
    std::io::Error::other(format!("{step} failed: 0x{:08x}", result as u32))
}

/// Calls `release` on a COM object Aura holds.
fn release(object: Raw) {
    if !object.is_null() {
        // SAFETY: every object passed here is a live COM pointer Aura owns one reference to.
        unsafe { ((*(*(object as *mut *const UnknownVtbl))).release)(object) };
    }
}

/// Moves `path` (a file or a folder, by its final path) to the Recycle Bin on a thread of its own,
/// as the Windows shell requires; never deletes it for good.
pub fn move_to_recycle_bin(path: &Path) -> std::io::Result<RecycleOutcome> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    std::thread::spawn(move || recycle_on_this_thread(&wide))
        .join()
        .map_err(|_| std::io::Error::other("the Recycle Bin operation stopped unexpectedly"))?
}

fn recycle_on_this_thread(wide: &[u16]) -> std::io::Result<RecycleOutcome> {
    // SAFETY: a fresh thread joins an apartment of its own and leaves it at the end.
    let joined = unsafe {
        CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32)
    };
    if joined < 0 {
        return Err(failed("CoInitializeEx", joined));
    }
    let outcome = recycle_in_apartment(wide);
    // SAFETY: paired with the successful CoInitializeEx above.
    unsafe { CoUninitialize() };
    outcome
}

fn recycle_in_apartment(wide: &[u16]) -> std::io::Result<RecycleOutcome> {
    let mut operation: Raw = std::ptr::null_mut();
    // SAFETY: standard COM creation with valid ids and out pointer.
    let created = unsafe {
        CoCreateInstance(
            &FileOperation,
            std::ptr::null_mut(),
            CLSCTX_ALL,
            &IID_IFILE_OPERATION,
            &mut operation,
        )
    };
    if created < 0 || operation.is_null() {
        return Err(failed("CoCreateInstance(FileOperation)", created));
    }
    let mut item: Raw = std::ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated; out pointer valid.
    let parsed = unsafe {
        SHCreateItemFromParsingName(
            wide.as_ptr(),
            std::ptr::null_mut(),
            &IID_ISHELL_ITEM,
            &mut item,
        )
    };
    if parsed < 0 || item.is_null() {
        release(operation);
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    }
    let sink = Box::into_raw(Box::new(Sink {
        vtbl: &SINK_VTBL,
        references: AtomicU32::new(1),
        refused: AtomicBool::new(false),
        recycled: AtomicBool::new(false),
    }));
    let outcome = run(operation, item, sink as Raw);
    release(item);
    release(operation);
    // SAFETY: Aura's own reference to the sink, after Windows released its own.
    let refused = unsafe { &*sink }.refused.load(Ordering::Acquire);
    let recycled = unsafe { &*sink }.recycled.load(Ordering::Acquire);
    // SAFETY: drops Aura's reference; the sink frees itself when it is the last.
    unsafe { sink_release(sink as Raw) };
    let aborted = outcome?;
    if refused {
        Ok(RecycleOutcome::NotRecyclable)
    } else if recycled && !aborted {
        Ok(RecycleOutcome::Recycled)
    } else {
        Err(std::io::Error::other("the Recycle Bin did not take the item"))
    }
}

/// Advise, flag, delete, perform; whether anything was aborted.
fn run(operation: Raw, item: Raw, sink: Raw) -> std::io::Result<bool> {
    // SAFETY: `operation` is a live IFileOperation; its table is ShObjIdl's.
    let table = unsafe { &**(operation as *mut *const FileOperationVtbl) };
    let mut cookie = 0_u32;
    // SAFETY: valid live objects for every call below.
    unsafe {
        let advised = (table.advise)(operation, sink, &mut cookie);
        if advised < 0 {
            return Err(failed("IFileOperation::Advise", advised));
        }
        let flags = FOF_NO_UI | FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE;
        let flagged = (table.set_operation_flags)(operation, flags);
        let mut result = flagged;
        if result >= 0 {
            result = (table.delete_item)(operation, item, std::ptr::null_mut());
        }
        if result >= 0 {
            // A cancelled item makes the whole operation report a failure: that is expected.
            let _ = (table.perform_operations)(operation);
        }
        let mut aborted = 0_i32;
        let _ = (table.get_any_operations_aborted)(operation, &mut aborted);
        let _ = (table.unadvise)(operation, cookie);
        let _ = (table.add_ref, table.release, table.query_interface);
        if flagged < 0 {
            return Err(failed("IFileOperation::SetOperationFlags", flagged));
        }
        if result < 0 {
            return Err(failed("IFileOperation::DeleteItem", result));
        }
        Ok(aborted != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The callback's one decision: an item Windows would recycle goes; one it would delete for
    /// good is cancelled and stays.
    #[test]
    fn only_an_item_windows_would_recycle_is_let_go() {
        assert_eq!(decide(TSF_DELETE_RECYCLE_IF_POSSIBLE as u32), RecycleOutcome::Recycled);
        assert_eq!(decide(TSF_DELETE_RECYCLE_IF_POSSIBLE as u32 | 0x1), RecycleOutcome::Recycled);
        assert_eq!(decide(0), RecycleOutcome::NotRecyclable);
        let sink = Box::into_raw(Box::new(Sink {
            vtbl: &SINK_VTBL,
            references: AtomicU32::new(1),
            refused: AtomicBool::new(false),
            recycled: AtomicBool::new(false),
        }));
        // SAFETY: the sink is live until its last reference is released below.
        unsafe {
            assert_eq!(pre_delete(sink as Raw, 0, std::ptr::null_mut()), E_ABORT);
            assert!((*sink).refused.load(Ordering::Acquire));
            assert_eq!(
                pre_delete(
                    sink as Raw,
                    TSF_DELETE_RECYCLE_IF_POSSIBLE as u32,
                    std::ptr::null_mut()
                ),
                S_OK
            );
            assert_eq!(sink_release(sink as Raw), 0);
        }
    }

    /// The real Recycle Bin: run by hand (`--ignored`), it leaves its one small test file there.
    #[test]
    #[ignore = "puts a test file into this user's Recycle Bin"]
    fn a_file_goes_to_the_recycle_bin_and_is_gone_from_its_folder() {
        let folder =
            std::env::temp_dir().join(format!("aura-l13-4-recycle-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let file = folder.join("aura-l13-4-recycle-test.txt");
        std::fs::write(&file, b"L13.4 recycle test").expect("file");
        let final_path = std::fs::canonicalize(&file).expect("final path");
        let plain = final_path.display().to_string();
        let plain = plain.strip_prefix(r"\\?\").unwrap_or(&plain).to_owned();
        assert_eq!(
            move_to_recycle_bin(Path::new(&plain)).expect("recycle"),
            RecycleOutcome::Recycled
        );
        assert!(!file.exists());
        std::fs::remove_dir(&folder).expect("empty folder");
    }
}
