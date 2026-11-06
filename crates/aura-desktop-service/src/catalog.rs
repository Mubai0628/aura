//! The selected desktop's catalog and icon cache.
use super::*;
impl DesktopControl {
    /// The programs of the Start menu and the ones the user added, read again at most every
    /// [`CATALOG_FOR`].
    pub async fn catalog(&self) -> Result<Vec<AppEntry>, HostError> {
        if let Err(error) = self.ensure_target().await {
            self.lock().catalog_issue = Some(error.message.clone());
            return Err(error);
        }
        if let Some((at, apps)) = &self.lock().catalog
            && at.elapsed() < CATALOG_FOR
        {
            return Ok(apps.clone());
        }
        let manual = self
            .settings()
            .settings
            .added
            .into_iter()
            .map(|app| ManualApp { name: app.name, path: app.path })
            .collect();
        match self.run(HostCommand::Catalog { manual }, CATALOG_LIMIT).await? {
            HostResult::Catalog { apps } => {
                self.lock().catalog = Some((Instant::now(), apps.clone()));
                self.lock().catalog_issue = None;
                Ok(apps)
            }
            other => Err(unexpected("列程序", &other)),
        }
    }

    /// Forgets the catalog; the next listing reads it again (after the user added a program).
    pub fn refresh_catalog(&self) {
        self.lock().catalog = None;
        if let Some(child) = self.child.get() {
            child.refresh_catalog();
        }
    }

    /// The icons of the catalog's programs among `ids`, each read from Windows once while Core
    /// runs; an ID the catalog does not have and a program Windows has no icon for are left out.
    pub async fn icons(&self, ids: &[String]) -> Result<Vec<AppIcon>, HostError> {
        let catalog = self.catalog().await?;
        let wanted: Vec<&AppEntry> =
            ids.iter().filter_map(|id| catalog.iter().find(|app| &app.id == id)).collect();
        let _reading = self.icon_reads.lock().await;
        let missing: Vec<AppEntry> = {
            let state = self.lock();
            wanted
                .iter()
                .filter(|app| !state.icons.contains_key(&app.id))
                .map(|&app| app.clone())
                .collect()
        };
        for apps in missing.chunks(ICONS_PER_READ) {
            let command = HostCommand::Icons { apps: apps.to_vec(), size: ICON_SIZE };
            let icons = match self.run(command, ICONS_LIMIT).await? {
                HostResult::Icons { icons } => icons,
                other => return Err(unexpected("读图标", &other)),
            };
            let mut state = self.lock();
            for app in apps {
                state.icons.insert(app.id.clone(), None);
            }
            for icon in icons {
                state.icons.insert(icon.app_id, Some(icon.png));
            }
        }
        let state = self.lock();
        Ok(wanted
            .iter()
            .filter_map(|app| {
                let png = state.icons.get(&app.id)?.clone()?;
                Some(AppIcon { app_id: app.id.clone(), png })
            })
            .collect())
    }
}
