//! Programs on the user's own desktop that the user allowed, through Aura's desktop host. Nothing
//! here reaches a program the call does not name, and every program asks the user once per
//! conversation unless the user always allows it.
use super::definitions::entry;
use super::*;
use AgentBuiltinHandler as H;
use aura_desktop_domain::*;
use aura_security_contract::ResolvedCapability as C;

const WHEN: &str = "When the user referred to a program with @ or asked you to operate a program, operate it with the computer tools. When the user did not say how to do a task, choose the way that costs least time and tokens: a program's files, a command line, an MCP server or a plugin is usually quicker than operating its window. Before operating a program the user did not ask you to operate, say why; the user is then asked to allow the program for this conversation. Every call waits for a model step, which can take a minute: unless the user said otherwise, do in one call the steps you already know, take the screenshot with the action that needs one, and make calls that do not depend on each other together.";
const APP: &str = "app is the program's id from computer.apps.";
const SAFE: &str = "Never type or set a value into a password field. Never pay, transfer money, trade or buy. When a result says it is not known whether a step was carried out, read the program again before deciding; never repeat the step on your own.";

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    let read = &[C::DesktopRead];
    let act = &[C::DesktopInput];
    [
        entry::<ComputerAppsInput, ComputerAppsResult>(
            110,
            H::ComputerApps,
            "List the programs on the user's computer",
            &format!("List the programs computer control can operate: the Start menu's All apps and the programs the user added in Settings, with the id each of the other computer tools takes, whether each has a window now (running) and whether this conversation may already use it without asking (allowed). query keeps those whose name or id contains it. Listing reads no program. {WHEN}"),
            // The host's list of programs and windows, read before admission; no program is read.
            &[C::AgentTaskRead],
            false,
            at,
        ),
        entry::<ComputerOpenInput, ComputerOpenResult>(
            111,
            H::ComputerOpen,
            "Open a program",
            &format!("Open the program and bring its window to the front. A program that has a window already is not opened again: that window comes to the front. Returns its windows with refs (w1312) for the other tools. {APP} {WHEN}"),
            act,
            true,
            at,
        ),
        entry::<ComputerSnapshotInput, ComputerSnapshotResult>(
            112,
            H::ComputerSnapshot,
            "Read a program's window",
            &format!("Read one of the program's windows (window, or the one in front) as UI Automation shows it, element by element, the way screen readers do: one line each with a ref (e12) for computer.act and computer.input, its role, name, value, states and what can be done to it. Only what is on the screen and can be used or read is listed; an element marked more has more inside: read it with root set to its ref. Password fields show no value. The window stays where it is; a minimized one has nothing on the screen: restore it with computer.window first. Read before acting, and read again after the window changed: an old ref then says it is stale. {APP} {WHEN}"),
            read,
            false,
            at,
        ),
        entry::<ComputerScreenshotInput, ComputerScreenshotResult>(
            113,
            H::ComputerScreenshot,
            "Take a screenshot of a program",
            &format!("Capture the area around the program's windows on the screen, its menus and dialogs included (the whole screen when none of them is on it), with every window that is not the program's painted over, and return it with a ref (p3) whose pixels computer.input's points are given in. region takes a part of an earlier screenshot again at full detail. With marks, the elements of the program's latest computer.snapshot that can be acted on are boxed and numbered on the image: the box numbered n is the element en, for computer.act and computer.input; read the window first, and again after it changed. The screenshot is sent to the model's provider, and the user can see it with this call. A model that accepts images receives the image. Prefer computer.snapshot: look at the screen when the window's elements cannot be read or an action had no effect. {APP} {WHEN}"),
            read,
            false,
            at,
        ),
        entry::<ComputerActInput, ComputerActResult>(
            114,
            H::ComputerAct,
            "Act on a program's elements",
            &format!("Act on elements of an earlier computer.snapshot, step by step, without the pointer or the keyboard: click (invoke), set_value, check, uncheck, expand, collapse, select, focus, scroll_into_view and scroll. The program can stay in the background, and the user can go on working meanwhile; clicking a button or a check box may bring its window to the front, and focus does. The steps stop at the first that fails, and the result says how many were done and how each element is afterwards. A step whose element changed since the read is refused as stale: read again. screenshot takes one afterwards. {APP} {SAFE} {WHEN}"),
            act,
            true,
            at,
        ),
        entry::<ComputerInputInput, ComputerInputResult>(
            115,
            H::ComputerInput,
            "Use the mouse and keyboard on a program",
            &format!("Move the real mouse and type on the real keyboard in the program, step by step: click (button, count 2 for a double click), move, drag along a path, scroll, type text, press a key or chord (Enter, Control+s, Alt+F4) and wait. A point is x and y in the pixels of the screenshot named by image (p3), or the middle of an element of an earlier read (ref). The program's window comes to the front first, and every point must land on one of its windows: a window that moved since the screenshot is refused, and the step asks for a new screenshot. This takes over the user's mouse and keyboard: unless the user asked for them, use computer.act for the steps it can do. Chords the system answers (the Windows key, Alt+Tab, Ctrl+Alt+Delete) are refused unless the user allowed them in Settings. screenshot takes one afterwards. {APP} {SAFE} {WHEN}"),
            act,
            true,
            at,
        ),
        entry::<ComputerWindowInput, ComputerWindowResult>(
            116,
            H::ComputerWindow,
            "Arrange a program's window",
            &format!("Do one thing to one of the program's windows (window, or the one in front), as its title bar does: front brings it to the front, restoring it when it is minimized; minimize; maximize fills the screen and brings it to the front; restore gives it back its normal size without bringing it to the front; close asks it to close as its close button does. Returns the program's windows afterwards, and changed false when the window already was so. A window without a minimize or maximize button is refused that action. When a program asks whether to save as it closes, read it and ask the user unless they already said what to do; never choose not to save on your own. {APP} {SAFE} {WHEN}"),
            act,
            true,
            at,
        ),
    ]
    .into_iter()
    .collect()
}
