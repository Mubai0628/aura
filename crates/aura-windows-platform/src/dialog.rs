use std::{io, mem::size_of, path::PathBuf, ptr};

use windows_sys::Win32::UI::Controls::{
    Dialogs::{
        CommDlgExtendedError, GetSaveFileNameW, OFN_DONTADDTORECENT, OFN_EXPLORER, OFN_NOCHANGEDIR,
        OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    },
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION, TDF_SIZE_TO_CONTENT,
    TDF_USE_COMMAND_LINKS, TaskDialogIndirect,
};

const RESTART_BUTTON: i32 = 1_001;
const SAFE_MODE_BUTTON: i32 = 1_002;
const EXPORT_BUTTON: i32 = 1_003;
const EXIT_BUTTON: i32 = 1_004;
const ROLL_BACK_BUTTON: i32 = 1_005;
const START_WITHOUT_BACKUP_BUTTON: i32 = 1_006;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    Restart,
    SafeMode,
    ExportDiagnostics,
    RollBack,
    /// Starts a newer version whose data copy did not fit on the disk.
    StartWithoutBackup,
    Exit,
}

impl RecoveryAction {
    const fn button(self) -> i32 {
        match self {
            Self::Restart => RESTART_BUTTON,
            Self::SafeMode => SAFE_MODE_BUTTON,
            Self::ExportDiagnostics => EXPORT_BUTTON,
            Self::RollBack => ROLL_BACK_BUTTON,
            Self::StartWithoutBackup => START_WITHOUT_BACKUP_BUTTON,
            Self::Exit => EXIT_BUTTON,
        }
    }
}

/// What the recovery window says and offers. `previous_version` names the version
/// `RollBack` starts; the button is shown only when one is named.
pub struct RecoveryChoice<'a> {
    pub instruction: &'a str,
    pub details: &'a str,
    pub actions: &'a [RecoveryAction],
    pub previous_version: Option<&'a str>,
}

fn action_label(action: RecoveryAction, previous_version: Option<&str>) -> String {
    match action {
        RecoveryAction::Restart => "重新启动\n重新启动当前版本".to_owned(),
        RecoveryAction::SafeMode => "安全模式\n临时禁用外部扩展和 Web Runtime".to_owned(),
        RecoveryAction::ExportDiagnostics => "导出诊断\n保存本次启动与握手诊断".to_owned(),
        RecoveryAction::RollBack => {
            format!("回到上一版本\n启动 {}", previous_version.unwrap_or_default())
        }
        RecoveryAction::StartWithoutBackup => {
            "不备份，启动新版本\n以后回到上一版本时不能恢复更新前的数据".to_owned()
        }
        RecoveryAction::Exit => "退出\n保持应用关闭".to_owned(),
    }
}

pub fn show_recovery_dialog(choice: &RecoveryChoice<'_>) -> io::Result<RecoveryAction> {
    let title = wide_null("Aura 恢复");
    let instruction = wide_null(choice.instruction);
    let content = wide_null(choice.details);
    let actions: Vec<RecoveryAction> = choice
        .actions
        .iter()
        .copied()
        .filter(|action| *action != RecoveryAction::RollBack || choice.previous_version.is_some())
        .collect();
    let labels: Vec<Vec<u16>> = actions
        .iter()
        .map(|action| wide_null(&action_label(*action, choice.previous_version)))
        .collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = actions
        .iter()
        .zip(&labels)
        .map(|(action, label)| TASKDIALOG_BUTTON {
            nButtonID: action.button(),
            pszButtonText: label.as_ptr(),
        })
        .collect();
    let default = actions.first().copied().unwrap_or(RecoveryAction::Exit).button();
    let config = TASKDIALOGCONFIG {
        cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>())
            .map_err(|_| io::Error::other("TASKDIALOGCONFIG size overflow"))?,
        hwndParent: ptr::null_mut(),
        hInstance: ptr::null_mut(),
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_USE_COMMAND_LINKS | TDF_SIZE_TO_CONTENT,
        dwCommonButtons: 0,
        pszWindowTitle: title.as_ptr(),
        Anonymous1: Default::default(),
        pszMainInstruction: instruction.as_ptr(),
        pszContent: content.as_ptr(),
        cButtons: u32::try_from(buttons.len())
            .map_err(|_| io::Error::other("task dialog button count overflow"))?,
        pButtons: buttons.as_ptr(),
        nDefaultButton: default,
        cRadioButtons: 0,
        pRadioButtons: ptr::null(),
        nDefaultRadioButton: 0,
        pszVerificationText: ptr::null(),
        pszExpandedInformation: ptr::null(),
        pszExpandedControlText: ptr::null(),
        pszCollapsedControlText: ptr::null(),
        Anonymous2: Default::default(),
        pszFooter: ptr::null(),
        pfCallback: None,
        lpCallbackData: 0,
        cxWidth: 0,
    };
    let mut selected = EXIT_BUTTON;
    // SAFETY: all config pointers refer to live NUL-terminated strings and the
    // output pointer is valid for the synchronous duration of the dialog.
    let result = unsafe {
        TaskDialogIndirect(&raw const config, &mut selected, ptr::null_mut(), ptr::null_mut())
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(actions
        .iter()
        .copied()
        .find(|action| action.button() == selected)
        .unwrap_or(RecoveryAction::Exit))
}

pub fn choose_diagnostic_export_path(default_name: &str) -> io::Result<Option<PathBuf>> {
    let mut file = vec![0_u16; 32_768];
    let default = wide_null(default_name);
    let copy_length = default.len().min(file.len());
    file[..copy_length].copy_from_slice(&default[..copy_length]);
    let filter: Vec<u16> = "JSON 诊断文件\0*.json\0所有文件\0*.*\0\0".encode_utf16().collect();
    let title = wide_null("导出 Aura P01 诊断");
    let extension = wide_null("json");
    let mut dialog = OPENFILENAMEW {
        lStructSize: u32::try_from(size_of::<OPENFILENAMEW>())
            .map_err(|_| io::Error::other("OPENFILENAMEW size overflow"))?,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: u32::try_from(file.len())
            .map_err(|_| io::Error::other("file dialog buffer size overflow"))?,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_DONTADDTORECENT
            | OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_OVERWRITEPROMPT
            | OFN_PATHMUSTEXIST,
        lpstrDefExt: extension.as_ptr(),
        ..OPENFILENAMEW::default()
    };
    // SAFETY: `dialog` and every pointer it contains remain valid while the
    // modal function runs; its output buffer is writable and sized correctly.
    if unsafe { GetSaveFileNameW(&mut dialog) } == 0 {
        // SAFETY: CommDlgExtendedError has no preconditions and must be called
        // immediately after the failed common-dialog call.
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 { Ok(None) } else { Err(io::Error::from_raw_os_error(error as i32)) };
    }
    let length = file.iter().position(|unit| *unit == 0).unwrap_or(file.len());
    let path = String::from_utf16(&file[..length])
        .map_err(|_| io::Error::other("save path contains invalid UTF-16"))?;
    Ok(Some(PathBuf::from(path)))
}

pub(crate) fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
