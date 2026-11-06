//! What Core keeps of the tools' calls, in memory only: what each call was resolved to, so every
//! later resolution of the call reads the same; the programs a `computer.apps` call listed; and
//! the refs the model uses for elements (e12) and screenshots (p3). After Core restarts, an old
//! ref is unknown and the model reads the program again.
use aura_desktop_domain::{AppEntry, ElementKey, Rect, ScreenMap, WindowInfo};
use aura_domain_common::{SessionId, ToolCallId};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// The most calls whose resolution is kept; a call is resolved again only while it runs.
const PINNED: usize = 512;
/// The most `computer.apps` listings kept for their calls.
const LISTINGS: usize = 64;
/// The most element refs kept for one program of a session, the newest.
pub const ELEMENT_REFS: usize = 6_000;
/// The most screenshot refs kept for a session, the newest.
pub const SHOT_REFS: usize = 20;

/// A call as its first resolution froze it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pinned {
    pub action: serde_json::Value,
    pub app_id: String,
    pub authorized: bool,
}

/// A program a `computer.apps` call lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listed {
    pub entry: AppEntry,
    pub running: bool,
}

/// An element a read listed, by its ref.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownElement {
    pub window: String,
    pub key: ElementKey,
    pub rect: Rect,
    /// Something can be done to it, so a screenshot's marks number it.
    pub acts: bool,
}

/// A screenshot, by its ref: where its pixels are on the screen and where the program's windows
/// were when it was taken.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownShot {
    pub app_id: String,
    pub map: ScreenMap,
    pub windows: Vec<WindowInfo>,
}

/// Keys kept in the order they came, the oldest dropped past a limit.
struct Bounded<K, V> {
    values: HashMap<K, V>,
    order: VecDeque<K>,
    limit: usize,
}

impl<K: Copy + Eq + std::hash::Hash, V> Bounded<K, V> {
    fn new(limit: usize) -> Self {
        Self { values: HashMap::new(), order: VecDeque::new(), limit }
    }

    fn insert(&mut self, key: K, value: V) {
        if self.values.insert(key, value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > self.limit {
            if let Some(oldest) = self.order.pop_front() {
                self.values.remove(&oldest);
            }
        }
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.values.get(key)
    }
}

struct Numbered<V> {
    next: u32,
    values: BTreeMap<u32, V>,
    shared: Option<Arc<AtomicU32>>,
}

impl<V> Default for Numbered<V> {
    fn default() -> Self {
        Self { next: 0, values: BTreeMap::new(), shared: None }
    }
}

impl<V> Numbered<V> {
    fn add(&mut self, value: V, limit: usize) -> u32 {
        let next = match &self.shared {
            Some(shared) => shared
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |number| {
                    number.checked_add(1).filter(|number| *number < 1_000_000_000)
                })
                .ok()
                .map(|number| number + 1),
            None => self.next.checked_add(1).filter(|number| *number < 1_000_000_000),
        };
        let Some(next) = next else { return 0 };
        self.next = next;
        self.values.insert(self.next, value);
        while self.values.len() > limit {
            self.values.pop_first();
        }
        self.next
    }
}

pub(crate) struct Memory {
    numbers: Option<Arc<AtomicU32>>,
    inspections: Bounded<ToolCallId, (aura_desktop_domain::HostCommand, String)>,
    pinned: Bounded<ToolCallId, Pinned>,
    listings: Bounded<ToolCallId, Result<Vec<Listed>, String>>,
    elements: BTreeMap<(SessionId, String), Numbered<KnownElement>>,
    /// The numbers the latest read of each program handed out.
    latest: BTreeMap<(SessionId, String), std::ops::RangeInclusive<u32>>,
    shots: BTreeMap<SessionId, Numbered<KnownShot>>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            numbers: None,
            inspections: Bounded::new(PINNED),
            pinned: Bounded::new(PINNED),
            listings: Bounded::new(LISTINGS),
            elements: BTreeMap::new(),
            latest: BTreeMap::new(),
            shots: BTreeMap::new(),
        }
    }
}

/// The number of a ref such as e12 or p3; `None` for anything else.
#[must_use]
pub fn ref_number(text: &str, prefix: char) -> Option<u32> {
    let digits = text.trim().strip_prefix(prefix)?;
    if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|number| *number > 0)
}

impl Memory {
    pub(crate) fn with_numbers(numbers: Arc<AtomicU32>) -> Self {
        Self { numbers: Some(numbers), ..Self::default() }
    }
    pub(crate) fn clear(&mut self) {
        *self = Self { numbers: self.numbers.clone(), ..Self::default() };
    }
    pub(crate) fn inspect(
        &mut self,
        call: ToolCallId,
        command: aura_desktop_domain::HostCommand,
        safety: String,
    ) {
        self.inspections.insert(call, (command, safety));
    }

    pub(crate) fn pin(&mut self, call: ToolCallId, pinned: Pinned) {
        self.pinned.insert(call, pinned);
    }

    pub(crate) fn pinned(&self, call: ToolCallId) -> Option<Pinned> {
        self.pinned.get(&call).cloned()
    }

    pub(crate) fn remember_listing(
        &mut self,
        call: ToolCallId,
        listed: Result<Vec<Listed>, String>,
    ) {
        self.listings.insert(call, listed);
    }

    pub(crate) fn listing(&self, call: ToolCallId) -> Option<Result<Vec<Listed>, String>> {
        self.listings.get(&call).cloned()
    }

    /// Numbers the elements of a read of the program; the refs go on from the session's last read
    /// of it, so earlier refs keep meaning what they meant.
    pub(crate) fn remember_elements(
        &mut self,
        session: SessionId,
        app_id: &str,
        elements: Vec<KnownElement>,
    ) -> Vec<String> {
        let key = (session, app_id.to_owned());
        let table = self
            .elements
            .entry(key.clone())
            .or_insert_with(|| Numbered { shared: self.numbers.clone(), ..Numbered::default() });
        let first = table.next + 1;
        let refs: Vec<String> = elements
            .into_iter()
            .map(|element| format!("e{}", table.add(element, ELEMENT_REFS)))
            .collect();
        self.latest.insert(key, first..=table.next);
        refs
    }

    /// The elements of the session's latest read of the program, by number, as acts since left
    /// them.
    pub(crate) fn latest_read(&self, session: SessionId, app_id: &str) -> Vec<(u32, KnownElement)> {
        let key = (session, app_id.to_owned());
        let (Some(numbers), Some(table)) = (self.latest.get(&key), self.elements.get(&key)) else {
            return Vec::new();
        };
        // A read that listed nothing.
        if numbers.is_empty() {
            return Vec::new();
        }
        table
            .values
            .range(numbers.clone())
            .map(|(number, known)| (*number, known.clone()))
            .collect()
    }

    pub(crate) fn element(
        &self,
        session: SessionId,
        app_id: &str,
        reference: &str,
    ) -> Option<KnownElement> {
        let number = ref_number(reference, 'e')?;
        self.elements.get(&(session, app_id.to_owned()))?.values.get(&number).cloned()
    }

    /// What a ref stands for after an act changed its element.
    pub(crate) fn replace_element(
        &mut self,
        session: SessionId,
        app_id: &str,
        reference: &str,
        element: KnownElement,
    ) {
        let Some(number) = ref_number(reference, 'e') else { return };
        if let Some(known) = self
            .elements
            .get_mut(&(session, app_id.to_owned()))
            .and_then(|table| table.values.get_mut(&number))
        {
            *known = element;
        }
    }

    pub(crate) fn remember_shot(&mut self, session: SessionId, shot: KnownShot) -> String {
        let table = self
            .shots
            .entry(session)
            .or_insert_with(|| Numbered { shared: self.numbers.clone(), ..Numbered::default() });
        format!("p{}", table.add(shot, SHOT_REFS))
    }

    pub(crate) fn shot(&self, session: SessionId, reference: &str) -> Option<KnownShot> {
        let number = ref_number(reference, 'p')?;
        self.shots.get(&session)?.values.get(&number).cloned()
    }
}

/// A program named by a call, looked up in the catalog of the last listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FoundApp {
    Found(AppEntry),
    /// No program has that id, nor that name.
    Unknown,
    /// More than one program has that name; the call must give an id.
    Ambiguous(Vec<String>),
    /// The programs have not been listed since Core started.
    NotListed,
}

impl crate::DesktopControl {
    pub fn remember_inspection(
        &self,
        call: ToolCallId,
        command: aura_desktop_domain::HostCommand,
        safety: String,
    ) {
        self.memory().inspect(call, command, safety);
    }

    #[must_use]
    pub fn inspection(
        &self,
        call: ToolCallId,
        command: &aura_desktop_domain::HostCommand,
    ) -> Option<String> {
        self.memory()
            .inspections
            .get(&call)
            .filter(|(inspected, _)| inspected == command)
            .map(|(_, safety)| safety.clone())
    }

    /// Keeps what the call's first resolution read.
    pub fn pin(&self, call: ToolCallId, pinned: Pinned) {
        self.memory().pin(call, pinned);
    }

    #[must_use]
    pub fn pinned(&self, call: ToolCallId) -> Option<Pinned> {
        self.memory().pinned(call)
    }

    pub fn remember_listing(&self, call: ToolCallId, listed: Result<Vec<Listed>, String>) {
        self.memory().remember_listing(call, listed);
    }

    /// What the listing taken for a `computer.apps` call found.
    #[must_use]
    pub fn listing(&self, call: ToolCallId) -> Option<Result<Vec<Listed>, String>> {
        self.memory().listing(call)
    }

    /// The refs of a read's elements, in their order.
    pub fn remember_elements(
        &self,
        session: SessionId,
        app_id: &str,
        elements: Vec<KnownElement>,
    ) -> Vec<String> {
        self.memory().remember_elements(session, app_id, elements)
    }

    #[must_use]
    pub fn element(
        &self,
        session: SessionId,
        app_id: &str,
        reference: &str,
    ) -> Option<KnownElement> {
        self.memory().element(session, app_id, reference)
    }

    /// The elements of the session's latest read of the program, by their refs' numbers.
    #[must_use]
    pub fn latest_read(&self, session: SessionId, app_id: &str) -> Vec<(u32, KnownElement)> {
        self.memory().latest_read(session, app_id)
    }

    /// An act's element as it is now, for its ref.
    pub fn replace_element(
        &self,
        session: SessionId,
        app_id: &str,
        reference: &str,
        element: KnownElement,
    ) {
        self.memory().replace_element(session, app_id, reference, element);
    }

    pub fn remember_shot(&self, session: SessionId, shot: KnownShot) -> String {
        self.memory().remember_shot(session, shot)
    }

    #[must_use]
    pub fn shot(&self, session: SessionId, reference: &str) -> Option<KnownShot> {
        self.memory().shot(session, reference)
    }

    /// The program with this id, or else the one program with this name.
    #[must_use]
    pub fn find_app(&self, app: &str) -> FoundApp {
        let Some(catalog) = self.cached_catalog() else { return FoundApp::NotListed };
        if let Some(entry) = catalog.iter().find(|entry| entry.id == app) {
            return FoundApp::Found(entry.clone());
        }
        let wanted = app.trim().to_lowercase();
        let named: Vec<&AppEntry> =
            catalog.iter().filter(|entry| entry.name.to_lowercase() == wanted).collect();
        match named.as_slice() {
            [] => FoundApp::Unknown,
            [one] => FoundApp::Found((*one).clone()),
            many => FoundApp::Ambiguous(many.iter().map(|entry| entry.id.clone()).collect()),
        }
    }

    /// The catalog with whether each program has a window now.
    pub async fn listed(&self) -> Result<Vec<Listed>, aura_desktop_domain::HostError> {
        let apps = self.catalog().await?;
        let command = aura_desktop_domain::HostCommand::Windows { apps: apps.clone() };
        let windows = match self.run(command, WINDOWS_LIMIT).await? {
            aura_desktop_domain::HostResult::Windows { windows } => windows,
            other => {
                return Err(aura_desktop_domain::HostError::new(
                    aura_desktop_domain::HostErrorCode::Internal,
                    format!("列窗口时收到了别的结果：{other:?}"),
                    aura_desktop_domain::Delivery::NotDelivered,
                ));
            }
        };
        Ok(apps
            .into_iter()
            .map(|entry| Listed {
                running: windows
                    .iter()
                    .any(|window| window.app_id.as_deref() == Some(entry.id.as_str())),
                entry,
            })
            .collect())
    }
}

/// The time limit of listing the windows.
pub const WINDOWS_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);
