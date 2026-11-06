//! The programs' icons, for the composer's list and the cards that name a program.
use crate::native;
use aura_desktop_domain::{
    AppEntry, AppIcon, AppKind, Delivery, HostError, HostErrorCode, HostResult, MAX_ICON_APPS,
    MAX_ICON_SIZE,
};
use base64::Engine as _;

/// Each program's icon that Windows has, in the order asked; one without an icon is left out.
pub(super) fn icons(apps: &[AppEntry], size: u32) -> Result<HostResult, HostError> {
    if size == 0 || size > MAX_ICON_SIZE || apps.len() > MAX_ICON_APPS {
        return Err(HostError::new(
            HostErrorCode::InvalidRequest,
            format!("图标的大小要在 1–{MAX_ICON_SIZE} 像素之间，一次最多 {MAX_ICON_APPS} 个程序"),
            Delivery::NotDelivered,
        ));
    }
    let icons = apps
        .iter()
        .filter_map(|app| {
            let png = native::icon::icon(&parsing_name(app), size).ok()?;
            Some(AppIcon {
                app_id: app.id.clone(),
                png: base64::engine::general_purpose::STANDARD.encode(png),
            })
        })
        .collect();
    Ok(HostResult::Icons { icons })
}

/// Where the Shell finds a program's item: a program the user added is its file, the others are
/// items of All apps.
#[must_use]
pub fn parsing_name(app: &AppEntry) -> String {
    match app.kind {
        AppKind::Manual => app.id.clone(),
        AppKind::Packaged | AppKind::Desktop => format!("shell:AppsFolder\\{}", app.id),
    }
}
