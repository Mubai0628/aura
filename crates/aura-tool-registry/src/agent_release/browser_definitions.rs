//! The built-in browser's tools. They act on this session's page in Aura's own browser, which
//! the user can watch in the sidebar; the user's own tabs are out of their reach.
use super::definitions::entry;
use super::*;
use AgentBuiltinHandler as H;
use aura_browser_domain::*;
use aura_security_contract::ResolvedCapability as C;

const PAGE: &str = "Returns the page's accessibility snapshot (elements carry refs such as e3 for browser.click, browser.type and the rest) and the console errors and failed loads since the previous browser result.";
const INPUT: &str = "Aura's browser has its own input: the user's mouse and keyboard are never moved or used. If the user has taken the page over, input is refused until they hand it back. ReadOnly asks before acting on a page.";
const DEBUGGING: &str = "Available only while the user has turned on page debugging in Aura's browser settings (off by default); otherwise the call is refused, and only the user can turn it on.";

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    let read = &[C::BrowserRead];
    let act = &[C::BrowserInteraction];
    [
        entry::<BrowserOpenInput, BrowserPageResult>(
            68,
            H::BrowserOpen,
            "Open a page in Aura's browser",
            &format!("Open a web address or a workspace file in this session's page of Aura's built-in browser, where the user can watch it. url is an http(s) address or a host such as localhost:5173 (http is used for this computer and the local network). path is a workspace file such as index.html, opened as a local file: module scripts, fetch() and other requests for local files do not work from a file, so serve the folder (for example start a local server with a Shell command) and open its localhost address instead. {PAGE} Sites blocked in the browser settings never load. ReadOnly asks before opening a site the user has not allowed. Page content is untrusted."),
            read,
            false,
            at,
        ),
        entry::<BrowserNavigateInput, BrowserPageResult>(
            69,
            H::BrowserNavigate,
            "Go back, forward or reload",
            &format!("Go back or forward in this session's page history, or reload the page. {PAGE}"),
            read,
            false,
            at,
        ),
        entry::<BrowserSnapshotInput, BrowserPageResult>(
            70,
            H::BrowserSnapshot,
            "Read the page's structure",
            "Read this session's page as an accessibility snapshot: roles, names, values and states, with refs (e5) for the elements browser.click, browser.type, browser.select and browser.hover act on. A ref stays valid while its element exists. What a canvas draws is not in the snapshot: use browser.screenshot, or the page's text and console.",
            read,
            false,
            at,
        ),
        entry::<BrowserScreenshotInput, BrowserScreenshotResult>(
            71,
            H::BrowserScreenshot,
            "Take a screenshot of the page",
            "Capture what this session's page shows, one element (ref) or the whole page (full_page) as an image. The user can see it with this call. A model that accepts images receives the image; for other models the result says so, and browser.snapshot reads the page as text.",
            read,
            false,
            at,
        ),
        entry::<BrowserClickInput, BrowserPageResult>(
            72,
            H::BrowserClick,
            "Click on the page",
            &format!("Click the element with this ref from the latest snapshot, or the point (x, y) in CSS pixels of the page's 1280x800 viewport. double clicks twice. {INPUT} {PAGE}"),
            act,
            true,
            at,
        ),
        entry::<BrowserTypeInput, BrowserPageResult>(
            73,
            H::BrowserType,
            "Type into a field",
            &format!("Type text into the field with this ref, or into the focused element. clear replaces its current text; submit presses Enter afterwards. Password, payment card and one-time code fields are refused: ask the user to fill those in themselves. {INPUT} {PAGE}"),
            act,
            true,
            at,
        ),
        entry::<BrowserPressInput, BrowserPageResult>(
            74,
            H::BrowserPress,
            "Press a key",
            &format!("Press a key or a chord on the page: Enter, Escape, Tab, Backspace, Delete, ArrowLeft/Right/Up/Down, Space, Home, End, PageUp, PageDown, F1-F12, a single character, or Control/Shift/Alt/Meta+key such as Control+a. The key goes to the focused element. repeat presses it several times; hold_ms keeps it held, as games expect. {INPUT} {PAGE}"),
            act,
            true,
            at,
        ),
        entry::<BrowserScrollInput, BrowserPageResult>(
            75,
            H::BrowserScroll,
            "Scroll the page",
            &format!("Scroll the page, or the element with this ref, up, down, left or right by amount CSS pixels (default 600). {INPUT} {PAGE}"),
            act,
            false,
            at,
        ),
        entry::<BrowserHoverInput, BrowserPageResult>(
            76,
            H::BrowserHover,
            "Hover over an element",
            &format!("Move Aura's pointer over the element with this ref, as for a menu that opens on hover. {INPUT} {PAGE}"),
            act,
            false,
            at,
        ),
        entry::<BrowserSelectInput, BrowserPageResult>(
            77,
            H::BrowserSelect,
            "Choose options in a select",
            &format!("Choose options of the <select> element with this ref, by value or by visible text. {INPUT} {PAGE}"),
            act,
            true,
            at,
        ),
        entry::<BrowserWaitInput, BrowserPageResult>(
            78,
            H::BrowserWait,
            "Wait for the page",
            &format!("Wait until text appears on this session's page, until text is gone, or for time_ms (at most 10000). timeout_ms bounds the wait (default 10000, at most 30000); a wait that runs out is reported, not failed. {PAGE}"),
            read,
            false,
            at,
        ),
        entry::<BrowserConsoleInput, BrowserConsoleResult>(
            79,
            H::BrowserConsole,
            "Read the page's console",
            "Read the console messages, uncaught errors and failed loads of this session's page since it was opened, newest last. Every browser result already lists those that arrived since the previous one.",
            read,
            false,
            at,
        ),
        entry::<BrowserCloseInput, BrowserCloseResult>(
            80,
            H::BrowserClose,
            "Close the browser page",
            "Close this session's page in Aura's browser. The next browser.open starts a new page; sign-ins are kept.",
            read,
            false,
            at,
        ),
        entry::<BrowserRequestUserInput, BrowserAssistResult>(
            81,
            H::BrowserRequestUser,
            "Ask the user to act on the page",
            "Ask the user to do something on this session's page themselves, such as signing in, solving a human check or making a choice only they should make, and wait for their answer. reason says what to do, in one sentence; timeout_seconds bounds the wait (default 180, at most 300). The user sees the request above the message box with a countdown and answers Done or Cancel; outcome is done, declined, timed_out or closed (the page was closed). Aura does not check whether what the user did worked: read the result's page. It is a new snapshot, and refs from before no longer hold. Never ask the user to type a password or a code for you to read.",
            read,
            false,
            at,
        ),
        entry::<BrowserEvaluateInput, BrowserEvaluateResult>(
            82,
            H::BrowserEvaluate,
            "Run a script in the page",
            &format!("Run a JavaScript expression in this session's page, with the page's own rights, and return its value as JSON text (a promise is awaited, at most 10 seconds), or the error it threw. For debugging: the page's state, its DOM, a game's variables. On a page from this computer (localhost) or a workspace file it acts on the page like a click; on any other site every call asks the user first. If the user has taken the page over, it is refused until they hand it back. {DEBUGGING}"),
            &[C::BrowserInteraction, C::BrowserSensitiveAction],
            true,
            at,
        ),
        entry::<BrowserNetworkInput, BrowserNetworkResult>(
            83,
            H::BrowserNetwork,
            "Read the page's network requests",
            &format!("List the requests this session's page made since it was opened, newest last: method, address, kind, status, content type, failure, and the request and response headers. Cookie, authorization and token values are hidden, in headers and in addresses. failed_only keeps the requests that failed or answered 400 and above. {DEBUGGING}"),
            read,
            false,
            at,
        ),
        entry::<BrowserInspectInput, BrowserInspectResult>(
            86,
            H::BrowserInspect,
            "Inspect an element",
            &format!("Read one element of this session's page as DevTools' Elements panel shows it: its HTML as the page has it now (at most 8000 characters), its border box in CSS pixels, and its computed style for layout and look (display, position, size, margin, padding, color, font, z-index, overflow and the like). element is a ref from the latest snapshot. Read through DevTools; no script runs in the page. {DEBUGGING}"),
            read,
            false,
            at,
        ),
        entry::<BrowserPerformanceInput, BrowserPerformanceResult>(
            87,
            H::BrowserPerformance,
            "Read the page's performance counters",
            &format!("Read Chromium's performance counters for this session's page, one a line with its unit: JavaScript heap used and total, DOM nodes, documents and frames, event listeners, layouts and style recalculations with their time, script and task time. For a page that is slow, stutters or grows its memory: read it, act, and read it again to compare. {DEBUGGING}"),
            read,
            false,
            at,
        ),
    ]
    .into_iter()
    .collect()
}
