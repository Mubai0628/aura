//! Isolated Chromium headless for already-fetched HTML and staged scripts.
//!
//! Network policy for this short-lived process is the existing zero-capability
//! AppContainer token used by native sandbox launches (`SECURITY_CAPABILITIES`
//! with `CapabilityCount = 0`). Windows ALE denies TCP and UDP for that token,
//! including WebRTC ICE; Core Gateway remains the only retrieval of page and
//! script bytes, which are staged as local files. This path does not enforce
//! network by HTTP proxy, browser switches, or the accepted Dnscache exception.
use std::{
    fs,
    io::{self, Read},
    os::windows::io::{FromRawHandle, RawHandle},
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
        EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, PROCESS_INFORMATION, ResumeThread,
        STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, WaitForSingleObject,
    },
};

use crate::{
    handle::OwnedHandle,
    job::{
        create_kill_on_close_job, job_active_processes, quote_windows_argument, terminate_job,
        wait_for_job_empty, wide_null,
    },
    sandbox::{
        AppContainerProfile, NativeAclAccess, ProcessAttributeList, create_inheritable_pipe,
        delete_appcontainer_profile, grant_file_dacl, set_non_inheritable,
    },
};

mod launch;
use launch::spawn_job_process;
#[cfg(test)]
mod account_tests;
#[cfg(test)]
mod launch_tests;
#[cfg(test)]
mod network_probes;

const MAX_RENDERED_BYTES: usize = 8 * 1024 * 1024;
const STILL_ACTIVE: u32 = 259;
const REAP_TIMEOUT: Duration = Duration::from_secs(5);
/// The application-compatibility layer variable. A third party can inject a layer
/// into a user's shell — measured here with `DetectorsAppHealth`, whose shim makes
/// this browser exit successfully with no DOM at all — and it then reaches every
/// process the shell starts, Aura's own included. It is removed from the render
/// child's environment and from nowhere else: no other Aura launch boundary and no
/// general shell path changes, and the parent's own environment is never modified.
const COMPATIBILITY_LAYER_VARIABLE: &str = "__COMPAT_LAYER";

/// Why one render ended the way it did, in one bounded word.
///
/// "Unavailable" covered a page that never loaded, a browser that never exited, a
/// pipe that was not drained and a browser that exited successfully with nothing
/// to show. The last one is the failure this batch was reported as, and it looked
/// identical to a genuinely empty page; a caller that has to say which happened
/// needs the distinction to exist somewhere. It carries no page, script,
/// environment or credential content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderOutcome {
    Rendered,
    /// The browser did not exit inside the render budget.
    WaitTimeout,
    /// The browser exited, but its output pipes were not fully collected.
    OutputNotReaped,
    /// The browser exited successfully and produced no markup at all.
    EmptyDom,
    /// The browser exited with a non-zero status.
    NonZeroExit,
    /// The browser produced more than this path will hold.
    OutputTooLarge,
}

fn edge_render_gate() -> Arc<tokio::sync::Mutex<()>> {
    static GATE: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))).clone()
}

struct EdgeRenderSlot {
    _permit: tokio::sync::OwnedMutexGuard<()>,
}

fn take_edge_slot_blocking() -> EdgeRenderSlot {
    EdgeRenderSlot { _permit: edge_render_gate().blocking_lock_owned() }
}

#[derive(Debug, thiserror::Error)]
pub enum HeadlessRenderError {
    #[error("web.rendering_unavailable")]
    Unavailable,
    #[error("web.response_too_large")]
    ResponseTooLarge,
}

#[derive(Debug)]
pub struct HeadlessFinish {
    pub rendered: Result<RenderedHtml, HeadlessRenderError>,
    pub job_empty: bool,
    pub output_reaped: bool,
    /// Which bounded ending this was, so "the browser produced nothing" can be
    /// told apart from "the browser never finished" without reading its output.
    pub outcome: RenderOutcome,
}

impl HeadlessFinish {
    pub fn stop_confirmed(&self) -> bool {
        self.job_empty && self.output_reaped
    }

    pub fn into_rendered(self) -> Result<RenderedHtml, HeadlessRenderError> {
        self.rendered
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedHtml {
    pub html: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessScript {
    pub src: String,
    pub file_name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessPage {
    pub html: String,
    pub scripts: Vec<HeadlessScript>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NetworkIsolation {
    ZeroCapabilityAppContainer,
    #[cfg(test)]
    None,
}

pub struct RenderAbort {
    job: OwnedHandle,
}

impl RenderAbort {
    pub fn terminate(&self) -> Result<(), HeadlessRenderError> {
        terminate_job(&self.job, 1).map_err(map_io)
    }

    pub fn wait_empty(&self, timeout: Duration) -> Result<bool, HeadlessRenderError> {
        wait_for_job_empty(&self.job, timeout).map_err(map_io)
    }

    pub fn job_is_empty(&self) -> Result<bool, HeadlessRenderError> {
        Ok(job_active_processes(&self.job).map_err(map_io)? == 0)
    }

    /// Terminates this render Job and waits until Windows reports it empty.
    pub fn terminate_and_confirm(&self, timeout: Duration) -> Result<bool, HeadlessRenderError> {
        self.terminate()?;
        self.wait_empty(timeout)
    }
}

struct RenderWorkspace {
    root: PathBuf,
    profile_name: Option<String>,
}

impl RenderWorkspace {
    fn create(
        isolation: NetworkIsolation,
    ) -> Result<(Self, Option<AppContainerProfile>), HeadlessRenderError> {
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| HeadlessRenderError::Unavailable)?;
        let stamp = entropy.iter().fold(String::new(), |mut acc, byte| {
            acc.push_str(&format!("{byte:02x}"));
            acc
        });
        let root = std::env::temp_dir().join("aura-web-render").join(&stamp);
        fs::create_dir_all(root.join("profile")).map_err(map_io)?;
        let (profile, profile_name) = match isolation {
            NetworkIsolation::ZeroCapabilityAppContainer => {
                let name = format!("AuraWr{stamp}");
                let profile = AppContainerProfile::ensure(&name).map_err(map_io)?;
                if !profile.was_created() {
                    return Err(HeadlessRenderError::Unavailable);
                }
                let sid = profile.sid_string().map_err(map_io)?;
                grant_file_dacl(&root, &sid, NativeAclAccess::ReadWrite, true).map_err(map_io)?;

                (Some(profile), Some(name))
            }
            #[cfg(test)]
            NetworkIsolation::None => (None, None),
        };
        Ok((Self { root, profile_name }, profile))
    }

    fn page_path(&self) -> PathBuf {
        self.root.join("page.html")
    }

    fn profile_path(&self) -> PathBuf {
        self.root.join("profile")
    }
}

impl Drop for RenderWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        if let Some(name) = self.profile_name.take() {
            let _ = delete_appcontainer_profile(&name);
        }
    }
}

enum PipeRead {
    Bytes(Vec<u8>),
    TooLarge,
    Failed,
}

pub struct HeadlessRenderSession {
    job: OwnedHandle,
    process: OwnedHandle,
    stdout: Option<JoinHandle<PipeRead>>,
    stderr: Option<JoinHandle<PipeRead>>,
    stdout_too_large: Arc<AtomicBool>,
    _workspace: RenderWorkspace,
    _appcontainer: Option<AppContainerProfile>,
    _edge_slot: Option<EdgeRenderSlot>,
}

impl Drop for HeadlessRenderSession {
    fn drop(&mut self) {
        let _ = terminate_job(&self.job, 1);
        let _ = wait_for_job_empty(&self.job, REAP_TIMEOUT);
        let deadline = Instant::now() + REAP_TIMEOUT;
        let _ = take_pipe(&mut self.stdout, deadline);
        let _ = take_pipe(&mut self.stderr, deadline);
    }
}

impl HeadlessRenderSession {
    pub fn abort_handle(&self) -> Result<RenderAbort, HeadlessRenderError> {
        Ok(RenderAbort { job: self.job.try_clone().map_err(map_io)? })
    }

    pub fn abort(&self) -> Result<(), HeadlessRenderError> {
        terminate_job(&self.job, 1).map_err(map_io)
    }

    pub fn active_processes(&self) -> Result<u32, HeadlessRenderError> {
        job_active_processes(&self.job).map_err(map_io)
    }

    pub fn finish(mut self, timeout: Duration) -> HeadlessFinish {
        let deadline = Instant::now() + timeout;
        let timeout_ms = millis_until(deadline);
        // SAFETY: the process handle remains owned for the wait.
        let waited = unsafe { WaitForSingleObject(self.process.as_raw(), timeout_ms) };
        if waited != WAIT_OBJECT_0 {
            let _ = terminate_job(&self.job, 1);
        }
        let job_empty = match wait_for_job_empty(&self.job, remaining(deadline)) {
            Ok(true) => true,
            Ok(false) => {
                let _ = terminate_job(&self.job, 1);
                wait_for_job_empty(&self.job, remaining(deadline).max(Duration::from_millis(200)))
                    .unwrap_or(false)
            }
            Err(_) => false,
        };
        let (stdout, stdout_reaped) = take_pipe(&mut self.stdout, deadline);
        let (stderr, stderr_reaped) = take_pipe(&mut self.stderr, deadline);
        let _ = stderr;
        let output_reaped = stdout_reaped && stderr_reaped;
        let mut exit_code = 0_u32;
        let mut exit_captured = false;
        if job_empty && waited != WAIT_TIMEOUT {
            // SAFETY: the job is empty, so the root process handle is terminal.
            exit_captured =
                unsafe { GetExitCodeProcess(self.process.as_raw(), &mut exit_code) } != 0;
        }
        let (rendered, outcome) = if self.stdout_too_large.load(Ordering::SeqCst)
            || matches!(stdout, PipeRead::TooLarge)
        {
            (Err(HeadlessRenderError::ResponseTooLarge), RenderOutcome::OutputTooLarge)
        } else if !output_reaped {
            (Err(HeadlessRenderError::Unavailable), RenderOutcome::OutputNotReaped)
        } else if waited == WAIT_TIMEOUT
            || !job_empty
            || !exit_captured
            || exit_code == STILL_ACTIVE
        {
            // The browser never reached a terminal state this path can describe.
            (Err(HeadlessRenderError::Unavailable), RenderOutcome::WaitTimeout)
        } else if exit_code != 0 {
            (Err(HeadlessRenderError::Unavailable), RenderOutcome::NonZeroExit)
        } else {
            let PipeRead::Bytes(stdout) = stdout else {
                return HeadlessFinish {
                    rendered: Err(HeadlessRenderError::Unavailable),
                    job_empty,
                    output_reaped,
                    outcome: RenderOutcome::OutputNotReaped,
                };
            };
            if stdout.len() > MAX_RENDERED_BYTES {
                (Err(HeadlessRenderError::ResponseTooLarge), RenderOutcome::OutputTooLarge)
            } else {
                let stdout = String::from_utf8_lossy(&stdout);
                match stdout.find('<') {
                    Some(index) => {
                        let html = stdout[index..].trim().to_owned();
                        if html.is_empty() {
                            (Err(HeadlessRenderError::Unavailable), RenderOutcome::EmptyDom)
                        } else {
                            (Ok(RenderedHtml { html }), RenderOutcome::Rendered)
                        }
                    }
                    None => (Err(HeadlessRenderError::Unavailable), RenderOutcome::EmptyDom),
                }
            }
        };
        #[cfg(test)]
        if std::env::var_os("AURA_RENDER_DIAGNOSTICS").is_some() {
            eprintln!(
                "render diagnostic: waited={waited} job_empty={job_empty} stdout_reaped={stdout_reaped} stderr_reaped={stderr_reaped} exit_captured={exit_captured} outcome={outcome:?}"
            );
            // Bounded on purpose: the first kilobyte of the browser's own error
            // stream, never its environment, the page or any credential.
            if let PipeRead::Bytes(bytes) = &stderr {
                eprintln!(
                    "render stderr[{}]: {}",
                    bytes.len(),
                    String::from_utf8_lossy(&bytes[..bytes.len().min(1024)])
                );
            }
        }
        HeadlessFinish { rendered, job_empty, output_reaped, outcome }
    }
}

/// Renders fetched HTML in OS Edge Chromium with an isolated profile and Job.
pub fn render_fetched_html(
    html: &str,
    timeout: Duration,
) -> Result<RenderedHtml, HeadlessRenderError> {
    render_fetched_page(&HeadlessPage { html: html.to_owned(), scripts: Vec::new() }, timeout)
}

pub fn render_fetched_page(
    page: &HeadlessPage,
    timeout: Duration,
) -> Result<RenderedHtml, HeadlessRenderError> {
    start_headless_render(page)?.finish(timeout).into_rendered()
}

pub fn start_headless_render(
    page: &HeadlessPage,
) -> Result<HeadlessRenderSession, HeadlessRenderError> {
    start_headless_render_with(
        page,
        NetworkIsolation::ZeroCapabilityAppContainer,
        Some(take_edge_slot_blocking()),
    )
}

/// Waits for the process-wide render capacity without occupying a Tokio worker.
/// Returns `Ok(None)` when cancelled before a process is created.
pub async fn start_headless_render_cancellable(
    page: HeadlessPage,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<Option<HeadlessRenderSession>, HeadlessRenderError> {
    if *cancel.borrow_and_update() {
        return Ok(None);
    }
    let permit = tokio::select! {biased;
        changed = cancel.changed() => {
            let _ = changed;
            if *cancel.borrow() {
                return Ok(None);
            }
            edge_render_gate().lock_owned().await
        }
        permit = edge_render_gate().lock_owned() => permit,
    };
    if *cancel.borrow() {
        return Ok(None);
    }
    let slot = EdgeRenderSlot { _permit: permit };
    tokio::task::spawn_blocking(move || {
        start_headless_render_with(&page, NetworkIsolation::ZeroCapabilityAppContainer, Some(slot))
    })
    .await
    .map_err(|_| HeadlessRenderError::Unavailable)?
    .map(Some)
}

fn start_headless_render_with(
    page: &HeadlessPage,
    isolation: NetworkIsolation,
    slot: Option<EdgeRenderSlot>,
) -> Result<HeadlessRenderSession, HeadlessRenderError> {
    start_headless_render_configured(page, isolation, slot, Some(4000), None)
}

fn start_headless_render_configured(
    page: &HeadlessPage,
    isolation: NetworkIsolation,
    slot: Option<EdgeRenderSlot>,
    virtual_time_budget_ms: Option<u32>,
    dump_timeout_ms: Option<u32>,
) -> Result<HeadlessRenderSession, HeadlessRenderError> {
    if page.html.len() > MAX_RENDERED_BYTES {
        return Err(HeadlessRenderError::ResponseTooLarge);
    }
    for script in &page.scripts {
        if script.bytes.len() > MAX_RENDERED_BYTES
            || script.file_name.contains(['/', '\\', ':', '\0'])
            || Path::new(&script.file_name).components().count() != 1
        {
            return Err(HeadlessRenderError::Unavailable);
        }
    }
    let (workspace, appcontainer) = RenderWorkspace::create(isolation)?;
    let mut replacements = Vec::new();
    for script in &page.scripts {
        let path = workspace.root.join(&script.file_name);
        fs::write(&path, &script.bytes).map_err(map_io)?;
        replacements.push((script.src.clone(), file_url(&path)?));
    }
    let html = rewrite_script_sources(&page.html, &replacements);
    fs::write(workspace.page_path(), html.as_bytes()).map_err(map_io)?;
    let edge = edge_executable()?;
    let url = file_url(&workspace.page_path())?;
    let profile = workspace.profile_path();
    let disable_features = match isolation {
        // Edge 153's Windows task-manager integration crashes in DoTasksUpdated
        // under this AppContainer (confirmed by the crash dump and symbols).
        // This ephemeral headless renderer needs no Windows task-manager tab
        // integration. Its OS token, Job and network isolation are unchanged.
        // Its Web Account Manager sign-in of the Windows account makes Windows
        // rewrite the user's Microsoft account credentials during the render;
        // Credential Manager calls overlapping those rewrites lose updates.
        NetworkIsolation::ZeroCapabilityAppContainer => {
            "Translate,MediaRouter,AcceptCHFrame,msWindowsTaskManager,msImplicitSignin,msOneAuthWAM,msForceOneAuthWAM"
        }
        // Positive-control launches only: file:// pages must still be able to
        // attempt loopback so the AppContainer deny has a contrast. Isolated
        // launches keep the default Chromium private-network behavior.
        #[cfg(test)]
        NetworkIsolation::None => {
            "Translate,MediaRouter,AcceptCHFrame,msWindowsTaskManager,msImplicitSignin,msOneAuthWAM,msForceOneAuthWAM,PrivateNetworkAccessSendPreflights,BlockInsecurePrivateNetworkRequests"
        }
    };
    let mut arguments = vec![
        "--headless=new".to_owned(),
        "--disable-gpu".to_owned(),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
        "--disable-extensions".to_owned(),
        "--disable-sync".to_owned(),
        "--disable-component-update".to_owned(),
        format!("--disable-features={disable_features}"),
        // Chromium cannot nest its own sandbox inside the OS AppContainer token
        // that enforces this process's network deny.
        "--no-sandbox".to_owned(),
    ];
    if virtual_time_budget_ms.is_none() {
        arguments.push("--allow-file-access-from-files".to_owned());
        arguments.push("--disable-hang-monitor".to_owned());
    }
    if let Some(timeout) = dump_timeout_ms {
        arguments.push(format!("--timeout={timeout}"));
    }
    if let Some(budget) = virtual_time_budget_ms {
        arguments.push(format!("--virtual-time-budget={budget}"));
    }
    arguments.push("--dump-dom".to_owned());
    arguments.push(format!("--user-data-dir={}", profile.display()));
    arguments.push(url);
    spawn_job_process(edge, &arguments, workspace.root.clone(), workspace, appcontainer, slot)
}

fn spawn_reader(
    handle: OwnedHandle,
    abort: OwnedHandle,
    too_large: Option<Arc<AtomicBool>>,
) -> Result<JoinHandle<PipeRead>, HeadlessRenderError> {
    let file = file_from_handle(handle);
    Ok(thread::spawn(move || read_capped(file, abort, too_large)))
}

fn read_capped(
    mut file: fs::File,
    abort: OwnedHandle,
    too_large: Option<Arc<AtomicBool>>,
) -> PipeRead {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => return PipeRead::Bytes(buf),
            Ok(n) => {
                if buf.len().saturating_add(n) > MAX_RENDERED_BYTES {
                    if let Some(flag) = &too_large {
                        flag.store(true, Ordering::SeqCst);
                    }
                    let _ = terminate_job(&abort, 1);
                    return PipeRead::TooLarge;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(_) => return PipeRead::Failed,
        }
    }
}

fn take_pipe(handle: &mut Option<JoinHandle<PipeRead>>, deadline: Instant) -> (PipeRead, bool) {
    let Some(join) = handle.take() else {
        return (PipeRead::Failed, false);
    };
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(join.join().unwrap_or(PipeRead::Failed));
    });
    match rx.recv_timeout(remaining(deadline).max(Duration::from_millis(50))) {
        Ok(read) => (read, true),
        Err(_) => (PipeRead::Failed, false),
    }
}

fn file_from_handle(handle: OwnedHandle) -> fs::File {
    // SAFETY: ownership of a unique pipe read handle is transferred to File.
    unsafe { fs::File::from_raw_handle(handle.into_raw() as RawHandle) }
}

fn size_of_startup() -> usize {
    std::mem::size_of::<STARTUPINFOEXW>()
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn millis_until(deadline: Instant) -> u32 {
    u32::try_from(remaining(deadline).as_millis()).unwrap_or(u32::MAX)
}

fn rewrite_script_sources(html: &str, replacements: &[(String, String)]) -> String {
    let mut rewritten = html.to_string();
    for (from, to) in replacements {
        rewritten = rewritten.replace(&format!("src=\"{from}\""), &format!("src=\"{to}\""));
        rewritten = rewritten.replace(&format!("src='{from}'"), &format!("src='{to}'"));
    }
    rewritten
}

fn edge_executable() -> Result<PathBuf, HeadlessRenderError> {
    const CANDIDATES: &[&str] = &[
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .ok_or(HeadlessRenderError::Unavailable)
}

fn file_url(path: &Path) -> Result<String, HeadlessRenderError> {
    let text = path.to_str().ok_or(HeadlessRenderError::Unavailable)?;
    let unified = text.replace('\\', "/");
    if unified.starts_with('/') {
        Ok(format!("file://{unified}"))
    } else {
        Ok(format!("file:///{unified}"))
    }
}

fn map_io(error: io::Error) -> HeadlessRenderError {
    #[cfg(test)]
    if std::env::var_os("AURA_RENDER_DIAGNOSTICS").is_some() {
        eprintln!("render diagnostic I/O: {error:?}");
    }
    let _ = error;
    HeadlessRenderError::Unavailable
}

#[cfg(test)]
mod tests {
    use super::network_probes::*;
    use super::*;
    use std::io::{self, Write};
    use std::net::TcpListener;

    const COMPUTED: &str = "JS-OK-9";
    const FROM_CHAR_CODE: &str = "String.fromCharCode(74,83,45,79,75,45,57)";

    fn computed_page(script: &str) -> String {
        format!(
            "<!doctype html><html><body><div id=\"app\">static</div><script>{script}</script></body></html>"
        )
    }

    fn fixture_kind_from_workspace() -> Option<String> {
        std::fs::read_to_string("fixture.txt").ok().map(|value| value.trim().to_owned())
    }

    #[test]
    #[allow(clippy::zombie_processes)]
    fn render_job_fixture_helper() {
        if std::env::var("AURA_RENDER_FIXTURE").ok().as_deref() == Some("child-hold") {
            let _ = io::stdout().write_all(b"holder-child\n");
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_secs(30));
            return;
        }
        // Runs the production render from a process whose own environment carries
        // the compatibility layer, and reports what the child environment Aura
        // builds looks like. The variable is inherited here (that is the whole
        // point) and must be absent from the block this process hands to Edge.
        if std::env::var("AURA_RENDER_FIXTURE").ok().as_deref() == Some("compat-render") {
            let inherited = std::env::var(COMPATIBILITY_LAYER_VARIABLE).unwrap_or_default();
            let probe = std::env::var("AURA_COMPAT_PROBE").unwrap_or_default();
            let block =
                crate::child_environment::inherited_block_without(&[COMPATIBILITY_LAYER_VARIABLE])
                    .expect("child environment block");
            let child = String::from_utf16_lossy(&block);
            let _ = writeln!(
                io::stdout(),
                "AURA_COMPAT inherited={inherited} dropped={} probe={}",
                !child
                    .to_uppercase()
                    .contains(&format!("{}=", COMPATIBILITY_LAYER_VARIABLE.to_uppercase())),
                child.contains(&format!("AURA_COMPAT_PROBE={probe}")),
            );
            let _ = io::stdout().flush();
            let page = computed_page(&format!(
                "document.getElementById('app').textContent={FROM_CHAR_CODE};"
            ));
            match render_fetched_html(&page, Duration::from_secs(25)) {
                Ok(rendered) => {
                    let _ = writeln!(
                        io::stdout(),
                        "AURA_COMPAT_RENDER ok len={} has_computed={}",
                        rendered.html.len(),
                        rendered.html.contains(COMPUTED)
                    );
                }
                Err(error) => {
                    let _ = writeln!(io::stdout(), "AURA_COMPAT_RENDER error={error:?}");
                }
            }
            let _ = io::stdout().flush();
            return;
        }
        match fixture_kind_from_workspace().as_deref() {
            Some("hold-pipe") => {
                let _ = io::stdout().write_all(b"holder-root\n");
                let _ = io::stdout().flush();
                let _child = crate::test_spawn::spawn(
                    std::process::Command::new(std::env::current_exe().expect("exe"))
                        .env("AURA_RENDER_FIXTURE", "child-hold")
                        .args([
                            "--exact",
                            "headless_web::tests::render_job_fixture_helper",
                            "--nocapture",
                        ]),
                )
                .expect("child holder");
                thread::sleep(Duration::from_millis(200));
            }
            Some("ignore-short-wait") => {
                thread::sleep(Duration::from_secs(30));
            }
            Some("oversized-stdout") => {
                let blob = vec![b'x'; MAX_RENDERED_BYTES + 1];
                let _ = io::stdout().write_all(&blob);
                let _ = io::stdout().flush();
            }
            _ => {}
        }
    }

    /// The v5 defect, end to end and without touching this process's own
    /// environment: a child is started with the compatibility layer injected, it
    /// proves it inherited the variable, it proves the block Aura builds for the
    /// browser no longer carries it, and the real production render then has to
    /// return markup. Before the launch boundary built its own environment this
    /// child inherited the layer into Edge, Edge exited 0 with no DOM, and the
    /// render was reported unavailable with nothing to point at.
    ///
    /// The three shapes are covered: the variable absent, present under its real
    /// spelling, and present in another case. A Unicode value set on the child
    /// proves the block carries non-ASCII text across the boundary.
    #[test]
    fn a_compatibility_layer_in_the_parent_environment_does_not_empty_the_render() {
        for (name, expected) in [
            (None, ""),
            (Some("__COMPAT_LAYER"), "DetectorsAppHealth"),
            (Some("__compat_layer"), "DetectorsAppHealth"),
        ] {
            let mut command = std::process::Command::new(std::env::current_exe().expect("exe"));
            command
                .env("AURA_RENDER_FIXTURE", "compat-render")
                .env("AURA_COMPAT_PROBE", "路径✓")
                .args(["--exact", "headless_web::tests::render_job_fixture_helper", "--nocapture"]);
            match name {
                Some(name) => command.env(name, "DetectorsAppHealth"),
                None => command.env_remove("__COMPAT_LAYER"),
            };
            let output = crate::test_spawn::output(&mut command).expect("fixture child");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains(&format!("AURA_COMPAT inherited={expected} ")),
                "the child must really carry the layer for this case ({name:?}): {stdout}"
            );
            assert!(
                stdout.contains("dropped=true probe=true"),
                "the browser's environment must drop the layer and keep Unicode text ({name:?}): {stdout}"
            );
            assert!(
                stdout.contains("AURA_COMPAT_RENDER ok") && stdout.contains("has_computed=true"),
                "the production render must return DOM with the layer inherited ({name:?}): {stdout}"
            );
        }
    }

    fn spawn_fixture(kind: &str) -> HeadlessRenderSession {
        let (workspace, profile) =
            RenderWorkspace::create(NetworkIsolation::None).expect("workspace");
        std::fs::write(workspace.root.join("fixture.txt"), kind).expect("fixture kind");
        let exe = std::env::current_exe().expect("exe");
        spawn_job_process(
            exe,
            &[
                "--exact".into(),
                "headless_web::tests::render_job_fixture_helper".into(),
                "--nocapture".into(),
            ],
            workspace.root.clone(),
            workspace,
            profile,
            None,
        )
        .expect("fixture process")
    }

    #[test]
    fn computed_javascript_text_is_not_in_the_source() {
        let html =
            computed_page(&format!("document.getElementById('app').textContent={FROM_CHAR_CODE};"));
        assert!(!html.contains(COMPUTED));
        let rendered =
            render_fetched_html(&html, Duration::from_secs(25)).expect("Windows 11 Edge headless");
        assert!(
            rendered.html.contains(COMPUTED),
            "dumped DOM missing computed text: {}",
            rendered.html
        );
    }

    #[test]
    fn non_javascript_script_type_does_not_run() {
        let html =
            computed_page(&format!("document.getElementById('app').textContent={FROM_CHAR_CODE};"))
                .replace("<script>", "<script type=\"text/plain\">");
        assert!(!html.contains(COMPUTED));
        let rendered =
            render_fetched_html(&html, Duration::from_secs(25)).expect("Windows 11 Edge headless");
        assert!(
            !rendered.html.contains(COMPUTED),
            "plain script must not produce computed text: {}",
            rendered.html
        );
        assert!(rendered.html.contains("static"));
    }

    #[test]
    fn external_same_origin_script_runs_from_staged_file() {
        let html = "<!doctype html><html><body><div id=\"app\"></div><script src=\"app.js\"></script></body></html>";
        let script = format!("document.getElementById('app').textContent={FROM_CHAR_CODE};");
        assert!(!html.contains(COMPUTED));
        assert!(!script.contains(COMPUTED));
        let rendered = render_fetched_page(
            &HeadlessPage {
                html: html.to_owned(),
                scripts: vec![HeadlessScript {
                    src: "app.js".into(),
                    file_name: "app.js".into(),
                    bytes: script.into_bytes(),
                }],
            },
            Duration::from_secs(25),
        )
        .expect("staged external script");
        assert!(rendered.html.contains(COMPUTED), "external script did not run: {}", rendered.html);
    }

    #[test]
    fn unauthorized_fetch_is_attempted_and_blocked_by_appcontainer() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("addr").port();
        let html = fetch_attempt_page(port);
        let peer = tcp_peer(listener);
        let rendered = render_real_time(
            &HeadlessPage { html, scripts: Vec::new() },
            NetworkIsolation::ZeroCapabilityAppContainer,
            Duration::from_secs(20),
        );
        let connected = peer.reached();
        match rendered {
            Ok(rendered) => {
                network_evidence(
                    "unauthorized_fetch",
                    "appcontainer",
                    "fetch",
                    Some(&rendered.html),
                    connected,
                    None,
                );
                assert!(
                    fetch_attempted(&rendered.html),
                    "JS fetch attempt was not recorded: {}",
                    rendered.html
                );
                assert!(
                    !rendered.html.contains("data-attempt=\"resolved\""),
                    "isolated fetch must not succeed: {}",
                    rendered.html
                );
                assert!(!connected, "AppContainer TCP reached loopback; DOM={}", rendered.html);
            }
            Err(error) => {
                network_evidence(
                    "unauthorized_fetch",
                    "appcontainer",
                    "fetch",
                    None,
                    connected,
                    Some(&error.to_string()),
                );
                panic!(
                    "not_exercised: isolated fetch produced no DOM, so this is not a deny proof; error={error:?} peer={connected}"
                );
            }
        }
    }

    #[test]
    fn unrestricted_fetch_is_the_positive_control() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().expect("addr").port();
        let html = fetch_attempt_page(port);
        let page = HeadlessPage { html, scripts: Vec::new() };
        let peer = tcp_peer(listener);
        let rendered =
            render_unrestricted(&page, Duration::from_secs(25)).expect("positive control");
        let connected = peer.reached();
        network_evidence(
            "unrestricted_fetch",
            "none",
            "fetch",
            Some(&rendered.html),
            connected,
            None,
        );
        assert!(
            fetch_attempted(&rendered.html),
            "positive-control JS did not run: {}",
            rendered.html
        );
        assert!(
            connected,
            "positive control must reach the loopback listener; DOM={}",
            rendered.html
        );
    }

    #[test]
    fn webrtc_stun_is_attempted_and_blocked_by_appcontainer() {
        let (socket, host, port) = stun_endpoint();
        let hold = TcpListener::bind("127.0.0.1:0").expect("hold");
        let hold_port = hold.local_addr().expect("hold addr").port();
        let page = rtc_isolated_page(&host, port, hold_port);
        let peer = udp_peer(socket);
        let rendered = render_real_time(
            &page,
            NetworkIsolation::ZeroCapabilityAppContainer,
            Duration::from_secs(25),
        )
        .expect("isolated webrtc render");
        let reached = peer.reached();
        network_evidence(
            "unauthorized_webrtc",
            "appcontainer",
            "RTCPeerConnection+stun",
            Some(&rendered.html),
            reached,
            None,
        );
        if rendered.html.contains("data-rtc=\"unavailable\"") {
            panic!(
                "not_exercised: WebRTC API missing on this Edge; cannot claim STUN blocked: {}",
                rendered.html
            );
        }
        assert!(
            rendered.html.contains("data-wait=\"fetch-hold\""),
            "isolated webrtc dump missing fetch-hold marker: {}",
            rendered.html
        );
        let attempted = rendered.html.contains("data-rtc=\"started\"")
            || rendered.html.contains("data-rtc=\"offer\"")
            || rendered.html.contains("data-rtc=\"error\"");
        assert!(attempted, "WebRTC attempt was not recorded: {}", rendered.html);
        assert!(!reached, "AppContainer UDP STUN reached loopback; DOM={}", rendered.html);
    }

    #[test]
    fn a_positive_control_queued_behind_another_render_still_reaches_its_peer() {
        // Renders take the browser slot one at a time. The peer listens from before this render
        // queues; the queue must not end it before the browser has started.
        let (socket, host, port) = stun_endpoint();
        let hold = TcpListener::bind("127.0.0.1:0").expect("hold");
        let hold_port = hold.local_addr().expect("hold addr").port();
        serve_delayed_http(hold, Duration::from_secs(6));
        let ahead = take_edge_slot_blocking();
        let peer = udp_peer(socket);
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_secs(13));
            drop(ahead);
        });
        let rendered = render_real_time(
            &HeadlessPage { html: rtc_hold_page(&host, port, hold_port), scripts: Vec::new() },
            NetworkIsolation::None,
            Duration::from_secs(25),
        )
        .expect("queued positive webrtc");
        release.join().expect("render ahead");
        assert!(
            peer.reached(),
            "a queued render's STUN must reach its peer; DOM={}",
            rendered.html
        );
    }

    #[test]
    fn unrestricted_webrtc_stun_is_the_positive_control() {
        let (socket, host, port) = stun_endpoint();
        let hold = TcpListener::bind("127.0.0.1:0").expect("hold");
        let hold_port = hold.local_addr().expect("hold addr").port();
        serve_delayed_http(hold, Duration::from_secs(6));
        let html = rtc_hold_page(&host, port, hold_port);
        let peer = udp_peer(socket);
        let rendered = render_real_time(
            &HeadlessPage { html, scripts: Vec::new() },
            NetworkIsolation::None,
            Duration::from_secs(25),
        )
        .expect("positive webrtc");
        let reached = peer.reached();
        network_evidence(
            "unrestricted_webrtc",
            "none",
            "RTCPeerConnection+stun",
            Some(&rendered.html),
            reached,
            None,
        );
        if rendered.html.contains("data-rtc=\"unavailable\"") {
            panic!(
                "not_exercised: WebRTC API missing on this Edge; cannot use STUN as a transport proof: {}",
                rendered.html
            );
        }
        assert!(
            rendered.html.contains("data-rtc=\"started\"")
                || rendered.html.contains("data-rtc=\"offer\"")
                || rendered.html.contains("data-rtc=\"error\""),
            "positive-control WebRTC JS did not run: {}",
            rendered.html
        );
        assert!(
            reached,
            "not_exercised: positive control STUN did not reach the UDP socket; isolation silence is not a deny proof; DOM={}",
            rendered.html
        );
    }

    #[test]
    fn oversized_dump_is_classified_while_reading() {
        let session = spawn_fixture("oversized-stdout");
        let result = session.finish(Duration::from_secs(10));
        assert!(
            matches!(result.rendered, Err(HeadlessRenderError::ResponseTooLarge)),
            "expected ResponseTooLarge, got {:?}",
            result.rendered
        );
    }

    #[test]
    fn abort_confirms_only_after_the_job_is_empty() {
        let html =
            computed_page(&format!("document.getElementById('app').textContent={FROM_CHAR_CODE};"));
        let session = start_headless_render(&HeadlessPage { html, scripts: Vec::new() })
            .expect("start render");
        let abort = session.abort_handle().expect("abort handle");
        assert!(abort.terminate_and_confirm(Duration::from_secs(5)).expect("confirm"));
        assert_eq!(session.active_processes().expect("count"), 0);
        assert!(matches!(
            session.finish(Duration::from_secs(5)).rendered,
            Err(HeadlessRenderError::Unavailable)
        ));
    }

    #[test]
    fn parent_exit_with_child_holding_the_pipe_stays_bounded() {
        let session = spawn_fixture("hold-pipe");
        let started = Instant::now();
        let result = session.finish(Duration::from_secs(2));
        assert!(started.elapsed() < Duration::from_secs(8), "finish hung joining pipes");
        assert!(matches!(result.rendered, Err(HeadlessRenderError::Unavailable)));
        assert_eq!(result.stop_confirmed(), result.job_empty && result.output_reaped);
    }

    #[test]
    fn wait_timeout_without_terminate_is_bounded_and_unconfirmed() {
        let session = spawn_fixture("ignore-short-wait");
        let abort = session.abort_handle().expect("abort");
        assert!(!abort.wait_empty(Duration::from_millis(150)).expect("wait"));
        assert!(session.active_processes().expect("count") >= 1);
        let started = Instant::now();
        let result = session.finish(Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(6), "timeout finish hung");
        assert!(matches!(result.rendered, Err(HeadlessRenderError::Unavailable)));
        assert!(abort.terminate_and_confirm(Duration::from_secs(5)).expect("cleanup"));
    }

    #[test]
    fn oversize_source_is_rejected_before_edge() {
        let html = "a".repeat(MAX_RENDERED_BYTES + 1);
        assert!(matches!(
            render_fetched_html(&html, Duration::from_secs(1)),
            Err(HeadlessRenderError::ResponseTooLarge)
        ));
    }

    #[test]
    fn map_io_stays_unavailable() {
        assert!(matches!(map_io(io::Error::other("closed")), HeadlessRenderError::Unavailable));
    }
}
