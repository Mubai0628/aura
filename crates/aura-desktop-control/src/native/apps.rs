//! The Start menu's All apps (the Shell's AppsFolder, what `Get-StartApps` reads).
use super::text::take_com_string;
use crate::catalog::StartMenuItem;
use windows::Win32::Storage::EnhancedStorage::PKEY_Link_TargetParsingPath;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2, KF_FLAG_DEFAULT,
    SHGetKnownFolderItem, SHGetKnownFolderPath, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::core::{GUID, Interface};

/// Every item of All apps, in the Shell's order. The calling thread must have joined COM.
pub fn start_menu() -> anyhow::Result<Vec<StartMenuItem>> {
    // SAFETY: COM calls on interfaces the Shell returned; each out-array slot is owned by the
    // Option it fills and released when the array is dropped.
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        let mut found = Vec::new();
        loop {
            let mut batch: [Option<IShellItem>; 64] = std::array::from_fn(|_| None);
            let mut fetched = 0_u32;
            items.Next(&mut batch, Some(&raw mut fetched))?;
            if fetched == 0 {
                break;
            }
            found.extend(batch.iter().take(fetched as usize).flatten().map(|item| describe(item)));
        }
        Ok(found)
    }
}

unsafe fn describe(item: &IShellItem) -> StartMenuItem {
    // SAFETY: the item came from the enumeration above and is alive for this call.
    unsafe {
        let name =
            item.GetDisplayName(SIGDN_NORMALDISPLAY).map(take_com_string).unwrap_or_default();
        let parsing_name = item
            .GetDisplayName(SIGDN_PARENTRELATIVEPARSING)
            .map(take_com_string)
            .unwrap_or_default();
        let target = item
            .cast::<IShellItem2>()
            .ok()
            .and_then(|item| item.GetString(&PKEY_Link_TargetParsingPath).ok())
            .map(take_com_string)
            .filter(|target| !target.trim().is_empty())
            .or_else(|| in_known_folder(&parsing_name));
        StartMenuItem { parsing_name, name, target }
    }
}

/// `{6D809377-6AF0-444B-8957-A3773F02200E}\Tencent\Weixin\Weixin.exe` with the known folder's
/// path filled in; `None` for anything else.
pub fn in_known_folder(parsing_name: &str) -> Option<String> {
    let rest = parsing_name.strip_prefix('{')?;
    let (guid, relative) = rest.split_once('}')?;
    let relative = relative.strip_prefix('\\')?;
    let guid = GUID::try_from(guid).ok()?;
    // SAFETY: a GUID on the stack and no token; the returned string is copied and freed.
    let folder = unsafe { SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None) }.ok()?;
    let folder = take_com_string(folder);
    (!folder.is_empty()).then(|| format!("{}\\{relative}", folder.trim_end_matches('\\')))
}
