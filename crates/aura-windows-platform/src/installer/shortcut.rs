//! Shell shortcuts (.lnk) through the shell's own ShellLink object, and the per-user folders they
//! go in. A shortcut may carry the window identity (System.AppUserModel.ID) of the program it
//! starts, so Windows treats it as that program's shortcut: a pin and the running window share
//! one taskbar button.
use std::{
    ffi::c_void,
    io,
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::{
    Win32::{
        Foundation::{S_FALSE, S_OK},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    },
    core::{GUID, HRESULT, PCWSTR, PWSTR},
};

use super::wide_null;

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x0002_1401_0000_0000_c000_0000_0000_0046);
const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x0002_14f9_0000_0000_c000_0000_0000_0046);
const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000_010b_0000_0000_c000_0000_0000_0046);
const IID_IPROPERTY_STORE: GUID = GUID::from_u128(0x886d_8eeb_8cf2_4446_8d02_cdba_1dbd_cf99);
/// PKEY_AppUserModel_ID.
const APP_USER_MODEL_ID: PropertyKey =
    PropertyKey { fmtid: GUID::from_u128(0x9f4c_2855_9f79_4b39_a8d0_e1d4_2de1_d5f3), pid: 5 };
const VT_LPWSTR: u16 = 31;

#[repr(C)]
struct PropertyKey {
    fmtid: GUID,
    pid: u32,
}

/// A PROPVARIANT holding a string: its type, three reserved words and the union, of which the
/// string pointer is the first field (the union is two pointers wide).
#[repr(C)]
struct StringVariant {
    vt: u16,
    reserved: [u16; 3],
    value: *const u16,
    rest: usize,
}

#[repr(C)]
struct IUnknownVtbl {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// IShellLinkW, in declaration order; the slots this module does not call are typed loosely.
#[repr(C)]
struct IShellLinkWVtbl {
    unknown: IUnknownVtbl,
    get_path: usize,
    get_id_list: usize,
    set_id_list: usize,
    get_description: usize,
    set_description: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_working_directory: usize,
    set_working_directory: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_arguments: usize,
    set_arguments: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_hotkey: usize,
    set_hotkey: usize,
    get_show_cmd: usize,
    set_show_cmd: usize,
    get_icon_location: usize,
    set_icon_location: unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> HRESULT,
    set_relative_path: usize,
    resolve: usize,
    set_path: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
}

/// IPersistFile, in declaration order.
#[repr(C)]
struct IPersistFileVtbl {
    unknown: IUnknownVtbl,
    get_class_id: usize,
    is_dirty: usize,
    load: usize,
    save: unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> HRESULT,
    save_completed: usize,
    get_cur_file: usize,
}

/// IPropertyStore, in declaration order.
#[repr(C)]
struct IPropertyStoreVtbl {
    unknown: IUnknownVtbl,
    get_count: usize,
    get_at: usize,
    get_value: usize,
    set_value:
        unsafe extern "system" fn(*mut c_void, *const PropertyKey, *const StringVariant) -> HRESULT,
    commit: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

struct Interface<V>(*mut *const V);

impl<V> Interface<V> {
    /// SAFETY: the pointer is a live COM interface pointer whose vtable has layout `V`.
    unsafe fn vtbl(&self) -> &V {
        // SAFETY: guaranteed by the caller of `Interface::from`.
        unsafe { &**self.0 }
    }
}

impl<V> Drop for Interface<V> {
    fn drop(&mut self) {
        // SAFETY: every COM vtable starts with IUnknown and this reference is released once.
        unsafe {
            let unknown = &**self.0.cast::<*const IUnknownVtbl>();
            (unknown.release)(self.0.cast());
        }
    }
}

fn hr(result: HRESULT, what: &str) -> io::Result<()> {
    if result >= 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("{what} failed: 0x{:08x}", result as u32)))
    }
}

struct Apartment(bool);

impl Apartment {
    fn enter() -> io::Result<Self> {
        // SAFETY: CoInitializeEx takes no pointers that outlive the call.
        let result = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if result == S_OK || result == S_FALSE {
            return Ok(Self(true));
        }
        // RPC_E_CHANGED_MODE: the thread already has another apartment, which serves too.
        if result as u32 == 0x8001_0106 {
            return Ok(Self(false));
        }
        hr(result, "CoInitializeEx").map(|()| Self(false))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx above on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

pub struct Shortcut<'a> {
    pub path: &'a Path,
    pub target: &'a Path,
    pub arguments: &'a str,
    pub working_directory: &'a Path,
    pub description: &'a str,
    pub icon: Option<&'a Path>,
    /// The window identity of the program it starts.
    pub app_id: Option<&'a str>,
}

/// Writes the shortcut file, replacing one already there.
pub fn create_shortcut(shortcut: &Shortcut<'_>) -> io::Result<()> {
    let _apartment = Apartment::enter()?;
    let mut raw: *mut c_void = ptr::null_mut();
    // SAFETY: CLSID/IID are valid GUIDs and the output pointer is live.
    hr(
        unsafe {
            CoCreateInstance(
                &CLSID_SHELL_LINK,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ISHELL_LINK_W,
                &mut raw,
            )
        },
        "CoCreateInstance(ShellLink)",
    )?;
    let link = Interface::<IShellLinkWVtbl>(raw.cast());
    let target = wide_null(&shortcut.target.to_string_lossy());
    let arguments = wide_null(shortcut.arguments);
    let directory = wide_null(&shortcut.working_directory.to_string_lossy());
    let description = wide_null(shortcut.description);
    // SAFETY: `link` is a live IShellLinkW; every string is NUL-terminated and outlives the call.
    unsafe {
        let vtbl = link.vtbl();
        hr((vtbl.set_path)(raw, target.as_ptr()), "IShellLinkW::SetPath")?;
        hr((vtbl.set_arguments)(raw, arguments.as_ptr()), "IShellLinkW::SetArguments")?;
        hr(
            (vtbl.set_working_directory)(raw, directory.as_ptr()),
            "IShellLinkW::SetWorkingDirectory",
        )?;
        hr((vtbl.set_description)(raw, description.as_ptr()), "IShellLinkW::SetDescription")?;
        if let Some(icon) = shortcut.icon {
            let icon = wide_null(&icon.to_string_lossy());
            hr((vtbl.set_icon_location)(raw, icon.as_ptr(), 0), "IShellLinkW::SetIconLocation")?;
        }
    }
    if let Some(app_id) = shortcut.app_id {
        set_app_id(&link, raw, app_id)?;
    }
    let mut persist_raw: *mut c_void = ptr::null_mut();
    // SAFETY: QueryInterface on the live link with a valid IID and output pointer.
    hr(
        unsafe { (link.vtbl().unknown.query_interface)(raw, &IID_IPERSIST_FILE, &mut persist_raw) },
        "QueryInterface(IPersistFile)",
    )?;
    let persist = Interface::<IPersistFileVtbl>(persist_raw.cast());
    if let Some(parent) = shortcut.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let path = wide_null(&shortcut.path.to_string_lossy());
    // SAFETY: `persist` is a live IPersistFile and the path is NUL-terminated.
    hr(unsafe { (persist.vtbl().save)(persist_raw, path.as_ptr(), 1) }, "IPersistFile::Save")
}

/// Puts the window identity in the link's property store; the link keeps a copy of the value.
fn set_app_id(link: &Interface<IShellLinkWVtbl>, raw: *mut c_void, app_id: &str) -> io::Result<()> {
    let mut store_raw: *mut c_void = ptr::null_mut();
    // SAFETY: QueryInterface on the live link with a valid IID and output pointer.
    hr(
        unsafe { (link.vtbl().unknown.query_interface)(raw, &IID_IPROPERTY_STORE, &mut store_raw) },
        "QueryInterface(IPropertyStore)",
    )?;
    let store = Interface::<IPropertyStoreVtbl>(store_raw.cast());
    let value = wide_null(app_id);
    let variant = StringVariant { vt: VT_LPWSTR, reserved: [0; 3], value: value.as_ptr(), rest: 0 };
    // SAFETY: `store` is a live IPropertyStore; the key and the variant, whose string is
    // NUL-terminated, outlive both calls, and the store copies the value it keeps.
    unsafe {
        hr(
            (store.vtbl().set_value)(store_raw, &APP_USER_MODEL_ID, &variant),
            "IPropertyStore::SetValue",
        )?;
        hr((store.vtbl().commit)(store_raw), "IPropertyStore::Commit")
    }
}

fn known_folder(id: &GUID) -> io::Result<PathBuf> {
    let mut path: PWSTR = ptr::null_mut();
    // SAFETY: the folder id is valid and the output pointer is live.
    let result =
        unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as u32, ptr::null_mut(), &mut path) };
    let folder = if result >= 0 && !path.is_null() {
        // SAFETY: the shell returned a NUL-terminated string.
        let length = (0..).take_while(|&index| unsafe { *path.add(index) } != 0).count();
        // SAFETY: `length` units precede the terminator.
        Ok(PathBuf::from(String::from_utf16_lossy(unsafe {
            std::slice::from_raw_parts(path, length)
        })))
    } else {
        Err(io::Error::other(format!("SHGetKnownFolderPath failed: 0x{:08x}", result as u32)))
    };
    // SAFETY: the shell allocated the string with the COM allocator; null is allowed.
    unsafe { CoTaskMemFree(path.cast()) };
    folder
}

/// The current user's Start menu Programs folder.
pub fn start_menu_programs() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_Programs)
}

/// The current user's desktop.
pub fn desktop_folder() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_Desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `bytes` hold `text` as UTF-16, as a link's property store keeps a string.
    fn holds(bytes: &[u8], text: &str) -> bool {
        let wide: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes.windows(wide.len()).any(|window| window == wide.as_slice())
    }

    #[test]
    fn a_shortcut_file_is_written_where_asked() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("中文 目录").join("Aura.lnk");
        create_shortcut(&Shortcut {
            path: &path,
            target: Path::new(r"C:\Windows\System32\notepad.exe"),
            arguments: "",
            working_directory: folder.path(),
            description: "Aura",
            icon: None,
            app_id: None,
        })
        .expect("shortcut");
        let bytes = std::fs::read(&path).expect("shortcut file");
        // A shell link starts with its header size and the ShellLink class id.
        assert_eq!(&bytes[..4], &[0x4c, 0, 0, 0]);
        assert_eq!(&bytes[4..8], &[0x01, 0x14, 0x02, 0x00]);
        assert!(!holds(&bytes, "Aura.aura.desktop"));
    }

    #[test]
    fn a_shortcut_carries_the_window_identity_it_is_given() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("Aura.lnk");
        let identity = "Aura.aura.desktop.test-shortcut";
        create_shortcut(&Shortcut {
            path: &path,
            target: Path::new(r"C:\Windows\System32\notepad.exe"),
            arguments: "",
            working_directory: folder.path(),
            description: "Aura",
            icon: None,
            app_id: Some(identity),
        })
        .expect("shortcut");
        assert!(holds(&std::fs::read(&path).expect("shortcut file"), identity));
    }

    #[test]
    fn the_users_shortcut_folders_are_found() {
        assert!(start_menu_programs().expect("programs").ends_with(r"Start Menu\Programs"));
        assert!(desktop_folder().expect("desktop").is_absolute());
    }
}
