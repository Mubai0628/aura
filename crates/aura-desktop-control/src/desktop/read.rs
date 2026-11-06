//! Reading a program's window through UI Automation: the elements on the screen, within the
//! read's limits and time.
use super::target::{not_responding, read_error, stale, target_window, window_error};
use super::worker::{Ran, on_own_thread};
use crate::elements::{self, Limits, Node, Tree, UiError, Walked};
use crate::native::uia::{Automation, UiElement};
use crate::plan::invalid;
use aura_desktop_domain::{AppEntry, Delivery, ElementKey, HostError, HostResult, window_handle};
use std::time::{Duration, Instant};

struct Uia<'a>(&'a Automation);

impl Tree for Uia<'_> {
    type Item = UiElement;

    fn children(&mut self, of: &UiElement) -> Result<Vec<(UiElement, Node)>, UiError> {
        self.0.children(of)
    }
}

pub struct Read {
    pub root: Option<ElementKey>,
    pub max_elements: u32,
    pub max_depth: u32,
    pub timeout_ms: u32,
}

pub fn read(app: &AppEntry, window: Option<&str>, request: Read) -> Result<HostResult, HostError> {
    elements::check_read(request.max_elements, request.max_depth, request.timeout_ms)
        .map_err(invalid)?;
    let target = target_window(app, window)?;
    let handle = window_handle(&target.window).unwrap_or_default();
    let timeout = Duration::from_millis(u64::from(request.timeout_ms));
    let name = app.name.clone();
    let ran = on_own_thread(timeout, move || {
        // Each call may take half the time; no node is opened in the last quarter, so that what
        // was read is still answered in time.
        let automation = Automation::new(timeout / 2).map_err(|error| read_error(&name, &error))?;
        let (window_element, window_node) =
            automation.window(handle).map_err(|error| window_error(&name, &error))?;
        let (start, start_name) = match request.root {
            None => (window_element, window_node.name),
            Some(key) => match automation.find(&window_element, &key.runtime_id) {
                Ok(Some((element, node))) if elements::same(&key, &node) => (element, node.name),
                Ok(_) => return Err(stale()),
                Err(error) => return Err(read_error(&name, &error)),
            },
        };
        let limits = Limits {
            max_elements: request.max_elements as usize,
            max_depth: request.max_depth as usize,
            deadline: Instant::now() + timeout * 3 / 4,
        };
        elements::walk(&mut Uia(&automation), &start, &start_name, &limits)
            .map_err(|error| read_error(&name, &error))
    });
    match ran {
        Ran::Done(Ok(Walked { elements, complete })) => {
            Ok(HostResult::Elements { window: target, elements, complete })
        }
        Ran::Done(Err(error)) => Err(error),
        Ran::TimedOut => {
            Err(not_responding(&app.name, Some(request.timeout_ms), Delivery::NotDelivered, None))
        }
    }
}
