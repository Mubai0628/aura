use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use notify::{
    Config, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileWatchBackend {
    Native,
    Polling,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileWatchEventKind {
    Create,
    Modify,
    Remove,
    RenamePair,
    /// A real mutation whose specific kind the backend could not classify. The
    /// event still names the paths it concerns, which is what a caller needs in
    /// order to describe the change locally, so it must not be mistaken for a lost
    /// event.
    UnknownMutation,
    /// A pure observation such as an access or metadata read. It names a path but
    /// reports no change.
    OtherMutation,
    /// The backend could not describe what happened, so the only safe recovery is
    /// a full rescan of the scope.
    RescanRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileWatchEvent {
    pub kind: FileWatchEventKind,
    pub paths: Vec<PathBuf>,
    pub detail: Option<String>,
}

#[derive(Debug, Error)]
pub enum FileWatcherError {
    #[error("failed to initialize or register workspace watcher: {0}")]
    Notify(#[from] notify::Error),
}

enum WatcherImpl {
    Native(RecommendedWatcher),
    Polling(PollWatcher),
}

impl WatcherImpl {
    fn watcher_mut(&mut self) -> &mut dyn Watcher {
        match self {
            Self::Native(watcher) => watcher,
            Self::Polling(watcher) => watcher,
        }
    }
}

pub struct WorkspaceFileWatcher {
    backend: FileWatchBackend,
    watcher: WatcherImpl,
    receiver: Receiver<FileWatchEvent>,
    /// The absolute directories currently registered with the backend.
    ///
    /// Keeping this is what makes an in-place update possible: `notify` cannot be
    /// asked what is registered, and watching a directory twice is an error rather
    /// than a no-op, so the watcher has to remember its own set.
    watched: BTreeSet<PathBuf>,
}

impl WorkspaceFileWatcher {
    pub fn start(
        directories: &[PathBuf],
        poll_interval: Duration,
    ) -> Result<Self, FileWatcherError> {
        let (sender, receiver) = mpsc::channel();
        // A directory that disappeared between the catalog listing and this
        // registration cannot be watched. Skipping it keeps the rest of the tree
        // watched; treating it as a failure used to drop the whole group, which the
        // workspace answers with a full rescan — so a subtree deletion raced its own
        // watcher and turned into a rebuild.
        let watchable = watchable(directories);
        if let Ok(mut native) = notify::recommended_watcher(handler(sender.clone())) {
            let mut native_failed = false;
            for directory in &watchable {
                if native.watch(directory, RecursiveMode::NonRecursive).is_err() {
                    native_failed = true;
                    break;
                }
            }
            if !native_failed {
                return Ok(Self {
                    backend: FileWatchBackend::Native,
                    watcher: WatcherImpl::Native(native),
                    receiver,
                    watched: watchable,
                });
            }
        }

        let config =
            Config::default().with_poll_interval(poll_interval).with_follow_symlinks(false);
        let mut polling = PollWatcher::new(handler(sender), config)?;
        for directory in &watchable {
            polling.watch(directory, RecursiveMode::NonRecursive)?;
        }
        Ok(Self {
            backend: FileWatchBackend::Polling,
            watcher: WatcherImpl::Polling(polling),
            receiver,
            watched: watchable,
        })
    }

    /// Brings the registered set to `directories` without replacing the channel, and
    /// reports which directories are newly watched.
    ///
    /// Replacing the whole watcher after every batch is what lost events: the old
    /// channel still buffered whatever arrived between the batch being drained and the
    /// replacement being registered, and dropping its receiver discarded that buffer
    /// with no trace. One channel for the life of the workspace removes the window
    /// instead of narrowing it — an event that arrives during the update is delivered
    /// later on the same receiver, not dropped.
    ///
    /// The returned directories were not watched a moment ago, so their contents may
    /// have changed without producing any event. The caller re-reads exactly those
    /// subtrees, which is the registration window this function cannot close by
    /// itself.
    pub fn set_directories(
        &mut self,
        directories: &[PathBuf],
    ) -> Result<Vec<PathBuf>, FileWatcherError> {
        let wanted = watchable(directories);
        let removed = self.watched.difference(&wanted).cloned().collect::<Vec<_>>();
        let added = wanted.difference(&self.watched).cloned().collect::<Vec<_>>();
        for directory in &removed {
            // A directory that is gone is no longer watched. An `unwatch` that fails
            // means the backend already forgot it, which is the same outcome, so this
            // is deliberately not an error: failing here would turn a normal deletion
            // into a full catalog rebuild.
            let _ = self.watcher.watcher_mut().unwatch(directory);
            self.watched.remove(directory);
        }
        for (index, directory) in added.iter().enumerate() {
            if let Err(error) =
                self.watcher.watcher_mut().watch(directory, RecursiveMode::NonRecursive)
            {
                // Leave the registration set exactly as it was found, so a caller that
                // recovers by rebuilding does not also have to reason about a partial
                // update.
                for registered in added.iter().take(index) {
                    let _ = self.watcher.watcher_mut().unwatch(registered);
                }
                return Err(FileWatcherError::Notify(error));
            }
        }
        self.watched.extend(added.iter().cloned());
        Ok(added)
    }

    #[must_use]
    pub const fn backend(&self) -> FileWatchBackend {
        self.backend
    }

    #[must_use]
    pub fn receiver(&self) -> &Receiver<FileWatchEvent> {
        &self.receiver
    }

    /// How many directories are registered with the backend right now.
    #[must_use]
    pub fn watched_directory_count(&self) -> usize {
        self.watched.len()
    }
}

/// The directories that can be watched right now.
fn watchable(directories: &[PathBuf]) -> BTreeSet<PathBuf> {
    directories.iter().filter(|directory| directory.is_dir()).cloned().collect()
}

fn handler(sender: Sender<FileWatchEvent>) -> impl FnMut(notify::Result<Event>) + Send + 'static {
    move |event| {
        let mapped = match event {
            Ok(event) => map_event(event),
            Err(error) => {
                let detail = error.to_string();
                FileWatchEvent {
                    kind: FileWatchEventKind::RescanRequired,
                    paths: error.paths,
                    detail: Some(detail),
                }
            }
        };
        let _ = sender.send(mapped);
    }
}

fn map_event(event: Event) -> FileWatchEvent {
    let kind = match event.kind {
        EventKind::Create(_) => FileWatchEventKind::Create,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            FileWatchEventKind::RenamePair
        }
        EventKind::Modify(_) => FileWatchEventKind::Modify,
        EventKind::Remove(_) => FileWatchEventKind::Remove,
        EventKind::Access(_) => FileWatchEventKind::OtherMutation,
        // An unclassified mutation still names its paths. Those paths are exactly
        // what a caller needs to re-read the affected directory, so this is a
        // describable change rather than a lost event; only an event that names
        // nothing forces a rescan.
        EventKind::Any | EventKind::Other if !event.paths.is_empty() => {
            FileWatchEventKind::UnknownMutation
        }
        EventKind::Any | EventKind::Other => FileWatchEventKind::RescanRequired,
    };
    FileWatchEvent { kind, paths: event.paths, detail: None }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use notify::{
        Event, EventKind,
        event::{AccessKind, CreateKind, ModifyKind, RemoveKind, RenameMode},
    };

    use super::{FileWatchEventKind, map_event};

    #[test]
    fn preserves_rename_pairs_instead_of_collapsing_them_to_modify() {
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from(r"C:\workspace\old.rs"))
            .add_path(PathBuf::from(r"C:\workspace\new.rs"));
        let mapped = map_event(event);
        assert_eq!(mapped.kind, FileWatchEventKind::RenamePair);
        assert_eq!(mapped.paths.len(), 2);
    }

    #[test]
    fn keeps_create_and_remove_distinct_for_recreate_sequences() {
        let created = map_event(
            Event::new(EventKind::Create(CreateKind::File))
                .add_path(PathBuf::from(r"C:\workspace\atomic.tmp")),
        );
        let removed = map_event(
            Event::new(EventKind::Remove(RemoveKind::File))
                .add_path(PathBuf::from(r"C:\workspace\atomic.tmp")),
        );
        assert_eq!(created.kind, FileWatchEventKind::Create);
        assert_eq!(removed.kind, FileWatchEventKind::Remove);
    }

    #[test]
    fn unknown_native_events_require_a_catalog_rescan() {
        let mapped = map_event(Event::new(EventKind::Other));
        assert_eq!(mapped.kind, FileWatchEventKind::RescanRequired);
    }

    #[test]
    fn an_unclassified_mutation_that_names_a_path_stays_describable() {
        // A backend that cannot classify the change still knows which path it
        // concerns, which is what a bounded local re-read needs. Treating this as
        // a lost event would rebuild the whole workspace for one file write.
        let mapped =
            map_event(Event::new(EventKind::Any).add_path(PathBuf::from(r"C:\workspace\one.txt")));
        assert_eq!(mapped.kind, FileWatchEventKind::UnknownMutation);
        assert_eq!(mapped.paths.len(), 1);

        let other =
            map_event(Event::new(EventKind::Other).add_path(PathBuf::from(r"C:\workspace\two.rs")));
        assert_eq!(other.kind, FileWatchEventKind::UnknownMutation);
    }

    #[test]
    fn an_access_event_is_an_observation_not_a_change() {
        let mapped = map_event(
            Event::new(EventKind::Access(AccessKind::Read))
                .add_path(PathBuf::from(r"C:\workspace\read-only.txt")),
        );
        assert_eq!(mapped.kind, FileWatchEventKind::OtherMutation);
    }
}
