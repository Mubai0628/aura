//! Authoritative route inventory. Main and Preload bindings are generated from
//! these declarations; authentication and domain authorization remain separate.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteOwner {
    Runtime,
    Workspace,
    Security,
    Model,
    Tool,
    Agent,
    Git,
    Package,
    Memory,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteSurface {
    Main,
    Core,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteAuth {
    AuthenticatedMain,
    SecretInput,
    TrustedRenderer,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteIdempotency {
    ReadOnly,
    CommandInvocation,
    DomainRequest,
    /// Ordered interactive input has effects, cannot be automatically replayed,
    /// and has no settled CommandInvocation result to retain or query.
    TransientInput,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteCancellation {
    None,
    RequestLedger,
    DomainOperation,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteDescriptor {
    pub method: &'static str,
    pub owner: RouteOwner,
    pub surface: RouteSurface,
    pub auth: RouteAuth,
    pub request_type: &'static str,
    pub response_type: &'static str,
    pub idempotency: RouteIdempotency,
    pub cancellation: RouteCancellation,
    pub test_only: bool,
}

macro_rules! route {
    ($method:literal,$owner:ident,$surface:ident,$auth:ident,$request:literal,$response:literal,$idempotency:ident,$cancel:ident) => {
        RouteDescriptor {
            method: $method,
            owner: RouteOwner::$owner,
            surface: RouteSurface::$surface,
            auth: RouteAuth::$auth,
            request_type: $request,
            response_type: $response,
            idempotency: RouteIdempotency::$idempotency,
            cancellation: RouteCancellation::$cancel,
            test_only: false,
        }
    };
}
mod agent;
mod agent_hooks;
mod agent_mcp;
mod agent_plugins;
mod agent_skills;
mod browser;
mod chrome;
mod git;
mod media;
mod memory;
mod model;
mod package;
mod runtime;
mod security;
mod terminal;
mod terminal_input;
mod tool;
mod ui;
mod workspace;

pub fn registry() -> impl Iterator<Item = &'static RouteDescriptor> {
    workspace::ROUTES
        .iter()
        .chain(security::ROUTES)
        .chain(model::ROUTES)
        .chain(agent::ROUTES)
        .chain(agent_hooks::ROUTES)
        .chain(agent_mcp::ROUTES)
        .chain(agent_plugins::ROUTES)
        .chain(agent_skills::ROUTES)
        .chain(git::ROUTES)
        .chain(package::ROUTES)
        .chain(memory::ROUTES)
        .chain(tool::ROUTES)
        .chain(runtime::ROUTES)
        .chain(terminal::ROUTES)
        .chain(terminal_input::ROUTES)
        .chain(ui::ROUTES)
        .chain(browser::ROUTES)
        .chain(chrome::ROUTES)
        .chain(media::ROUTES)
}
pub fn core_route(method: &str) -> Option<&'static RouteDescriptor> {
    registry().find(|route| route.surface == RouteSurface::Core && route.method == method)
}
pub fn owned_by(method: &str, owner: RouteOwner) -> bool {
    core_route(method).is_some_and(|route| route.owner == owner)
}
pub fn side_effecting(method: &str) -> bool {
    core_route(method).is_some_and(|route| route.idempotency != RouteIdempotency::ReadOnly)
}
pub fn records_command_outcome(method: &str) -> bool {
    core_route(method).is_some_and(|route| {
        matches!(
            route.idempotency,
            RouteIdempotency::CommandInvocation | RouteIdempotency::DomainRequest
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn l03_b0_terminal_keys_remain_authenticated_effects_without_command_history() {
        let route = core_route("terminal.user.write").expect("terminal input route");
        assert_eq!(route.auth, RouteAuth::AuthenticatedMain);
        assert!(side_effecting(route.method));
        assert_eq!(route.idempotency, RouteIdempotency::TransientInput);
        assert_eq!(route.cancellation, RouteCancellation::None);
        assert!(!records_command_outcome(route.method));
        assert!(records_command_outcome("terminal.user.close"));
        let transient: BTreeSet<_> = registry()
            .filter(|route| route.idempotency == RouteIdempotency::TransientInput)
            .map(|route| (format!("{:?}", route.surface), route.method))
            .collect();
        // The waiting question's touch is the other interactive input.
        assert_eq!(
            transient,
            [
                ("Main".into(), "terminal.user.write"),
                ("Core".into(), "terminal.user.write"),
                ("Main".into(), "agent.query.touch"),
                ("Core".into(), "agent.query.touch"),
                // Main is quitting, so the Store may record a normal exit.
                ("Core".into(), "runtime.prepare_exit"),
                // The browser host's answer to one command: it completes Core's wait and is
                // never replayed; the command it answers is the tool call's own record.
                ("Core".into(), "browser.host.result"),
                ("Core".into(), "media.host.result"),
            ]
            .into_iter()
            .collect()
        );
    }

    /// The audited renderer-reachable inventory, per module.
    ///
    /// Main is the surface the versioned, whitelisted preload bridge binds, so an
    /// entry here is reachable from the renderer. These numbers were audited
    /// against the declarations, against the generated
    /// `resources/ipc/routes.v1.json` (301 entries = this registry minus the single
    /// test-only driver route) and against the generated preload bindings.
    const MAIN_PER_MODULE: [(&str, usize); 19] = [
        // `agent.attachments.text` (ReadOnly, both surfaces), the preload's
        // `readAttachmentText`: the text of a text file one of the conversation's messages
        // carried, found by its artifact in that session. Where an outside attachment is now
        // (`agent.attachments.locate`) stays Core-only; the renderer never learns the place.
        ("media", 1),
        // Settings → 网络与浏览器: read and save the browser settings (ReadOnly and
        // CommandInvocation, both surfaces). They open no page; a model cannot reach them.
        // And read a screenshot of the conversation's own browser calls.
        ("browser", 3),
        // Settings → 网络与浏览器 → Chrome: status, the switch, a pairing code, forgetting the
        // paired extension, opening the extension's folder (Main only), and stopping everything
        // Chrome does for Aura; a conversation's shared tabs and the user's "done" for a tab
        // handed to them. None of them reaches a page or a model.
        ("chrome", 8),
        ("memory", 10),
        // desktop-only user terminals: explicit Main/Core binding, no model authority.
        ("terminal", 7),
        // `session.archive`, `.restore`, `.delete` (Main and Core) and the Main-only
        // `workspace.delete`, which issues the removal confirmation the user gave.
        ("workspace", 33),
        // B1: two read-only Core bridges plus the fixed official installation URL.
        ("git", 3),
        // The tool-package page. Imports take no path from the renderer: Main shows
        // its own dialog and sends Core the chosen path on the authenticated surface.
        ("package", 14),
        // 26 after `security.native_session_consent.list` and `.revoke`: the reusable
        // Native session consent is the one authorization that outlives its Operation,
        // so it has its own renderer-reachable read and withdrawal. Both are
        // `TrustedRenderer` on Main and `AuthenticatedMain` on Core, and both are
        // reachable from the preload as `listNativeSessionConsents` /
        // `revokeNativeSessionConsent`.
        // Adds `security.memory_session_consent.list` and `.revoke`: a ReadOnly
        // session's "allow for this session" on one memory Vault, listed and withdrawn
        // like the file approval. `TrustedRenderer` on Main, `AuthenticatedMain` on Core,
        // reached through the validated P04 transport as `listMemorySessionConsents` /
        // `revokeMemorySessionConsent`.
        ("security", 32),
        // Adds `models.provider.set_connections` (CommandInvocation, both surfaces), the
        // preload's `setModelProviderConnections`: a provider's default and backup connections,
        // saved at the revision the settings page showed, like `models.provider.rename`.
        ("model", 35),
        // Adds the read-only activity query on both audited surfaces.
        // It is in generated MAIN_ROUTES; Core retains authentication. The model
        // reaches it through Core tools without extending the fixed UI bridge.
        // Adds the local proxy handshake. It is TrustedRenderer on Main and
        // AuthenticatedMain on Core, ReadOnly, and reaches the preload as
        // `testAgentNetworkProxy`. It dials loopback only and starts no tool.
        // Adds the read-only `agent.commands.catalog` (`TrustedRenderer` on Main,
        // `AuthenticatedMain` on Core) as the preload's `listCommands`. It reads command
        // definitions only; running a command uses the existing routes. `agent.init.start`
        // (CommandInvocation) is the preload's `startWorkspaceInit`: `/init` starts a task
        // whose file write is approved like any other. Settings → 命令 adds
        // `agent.commands.set_enabled` and `.delete` (CommandInvocation, both surfaces; delete
        // removes only the user's own commands at a revision) and the Main-only
        // `agent.commands.open_directory`, which opens a folder named by Core's catalog.
        // Adds two Main-only routes (CommandInvocation): `agent.files.open_outside` opens
        // a workspace file with its default application (never a program or a script) or in the
        // file manager, and `agent.links.open` an http(s) page in the default browser. It also
        // adds the Main-only `agent.clipboard.write`: text the user copies goes to the clipboard
        // through Main, which refuses the browser's clipboard permissions to every page. It also
        // adds `agent.session.branch` (CommandInvocation, both surfaces): a new conversation
        // from one finished turn, carrying no operations, approvals or grants. It also adds
        // `agent.query.touch` (TransientInput, both surfaces): the user touched the waiting
        // question, so its countdown starts again; it is the preload's `touchAgentQuery`.
        // Adds the read-only `agent.sessions.activity` (both surfaces): which
        // conversations have a turn running or waiting for the user, for the sidebar; it is the
        // preload's `readAgentSessionsActivity`. It also adds `agent.compaction.start`
        // (CommandInvocation, both surfaces), the preload's `startAgentCompaction`: `/compact`
        // while nothing runs starts a one-off task, as `/init` does, that compacts the
        // conversation first; it carries a note and no tool or approval of its own. Adds
        // `agent.workspace.rebuild_index` (CommandInvocation, both surfaces), the preload's
        // `rebuildWorkspaceIndex`: a workspace's file index walked again after it failed.
        ("agent", 41),
        // Adds Settings → Hook in its own module: list, preview and invocation records
        // (ReadOnly), trust, untrust, enable, remove and reorder (CommandInvocation, both
        // surfaces), and the Main-only folder picker. None of them runs a Hook; trust is
        // given only at the fingerprint the user previewed.
        ("agent_hooks", 9),
        // Settings → MCP: list, preview and describe (ReadOnly); save, trust, untrust,
        // enable, remove and refresh (CommandInvocation; refresh starts the server to list its
        // tools); B5 adds setting a server's secret on the single-use `SecretInput` channel and
        // OAuth sign-in and sign-out (Main opens the sign-in address in the browser itself).
        // Adds putting a plugin server's own configuration back (CommandInvocation, both
        // surfaces), the preload's `restoreMcpPluginDefault`. None of them calls a server's tool.
        ("agent_mcp", 13),
        // Settings → Skill: list and preview (ReadOnly); trust, untrust, enable, import and
        // remove (CommandInvocation; import copies a folder into Aura's own, remove deletes only
        // such a copy), and three Main-only routes: the folder and ZIP file pickers and opening
        // a listed Skill's folder by its id. None of them loads a Skill for a task.
        ("agent_skills", 10),
        // Settings → 插件: list and preview (ReadOnly); import, trust, untrust, enable,
        // discard an update and uninstall (CommandInvocation; import copies a folder or unpacks
        // a ZIP file into Aura's own, uninstall deletes only such a copy), and three Main-only
        // routes: the folder and ZIP file pickers and opening an installed plugin's folder by
        // its name. None of them runs anything a plugin brings.
        ("agent_plugins", 11),
        // Adds the six tool-credential routes: list, outcome, clear and recover as
        // `TrustedRenderer`, create and rotate on the single-use `SecretInput` channel. All six
        // are bound by `preload/tool-credentials.ts`; the secret pair goes through its
        // `withSecret` path, never an ordinary request. The 17 old P07 Draft,
        // package and long-operation routes are retired; packages use the `package` module.
        ("tool", 17),
        // Adds the three configuration-recovery routes (notice, plan, apply) as
        // `TrustedRenderer`, bound by the preload's `invokeRecovery`; apply binds the previewed
        // plan, and none of them is reachable by a model tool.
        ("runtime", 10),
        ("terminal_input", 5),
        // 设置 → 环境: read (ReadOnly) and save (CommandInvocation, at the revision shown)
        // the interface settings — the session's debugging information and the default
        // terminal. `TrustedRenderer` on Main, `AuthenticatedMain` on Core; they start nothing.
        ("ui", 2),
    ];

    /// The two surfaces are not symmetric, and the difference is deliberate. Every
    /// method that reaches the renderer is a Main entry; these are the ones that
    /// exist only there, because Electron main or the renderer performs them without
    /// a Core command: the native directory dialog and folder operations, the local
    /// read models, and attaching the file the user picked as a read-only external
    /// grant.
    const MAIN_ONLY: [&str; 24] = [
        "agent.commands.open_directory",
        // Main opens the Chrome extension's folder in the file manager.
        "chrome.extension.open_folder",
        "agent.files.open_outside",
        "agent.links.open",
        "agent.clipboard.write",
        "agent.hooks.select_directory",
        "agent.plugins.open_directory",
        "agent.plugins.select_archive",
        "agent.plugins.select_directory",
        "agent.skills.open_directory",
        "agent.skills.select_archive",
        "agent.skills.select_directory",
        "memory.vault.open_directory",
        "memory.maintenance.open_log",
        "memory.vault.export",
        "git.installation.open",
        "core.health.get",
        "security.external_grant.attach_read_only",
        "workspace.archive",
        "workspace.delete",
        "workspace.relocate",
        "workspace.select_directory",
        "workspace_files.get",
        "workspace_session.get",
    ];

    /// The Core surface holds the authoritative commands. These have no renderer
    /// entry: internal broker channels, the runtime ledger, host preparation,
    /// and lifecycle handshakes. Git reads use the authenticated Main bridge. The
    /// test-only contract-driver route is not in the registry at all unless the
    /// `p08-contract-driver` feature is enabled, so it is not listed here — that is
    /// also why the generated inventory is one entry smaller than the declarations.
    const CORE_ONLY: [&str; 23] = [
        "media.host.read",
        "media.host.result",
        // Where an attached file outside the workspace is now, for Main to stream or open
        // it; the window names the attachment and never learns the place.
        "agent.attachments.locate",
        "agent.attachments.entry",
        "agent.files.mentioned",
        "memory.export.prepare",
        "memory.export.entry",
        "memory.export.chunk",
        "memory.export.close",
        "aura.cancel",
        "broker.context",
        "broker.http_read",
        "broker.tools",
        "runtime.command_outcome",
        // Main says it is quitting before it stops Core, so the Store may record a
        // normal exit; the window never asks for it.
        "runtime.prepare_exit",
        // Main is the browser host; it answers Core's browser command here. The window has
        // no way to it.
        "browser.host.result",
        "security.external_grant.attach_read_only_selected",
        "security.host_preparation.probe",
        "security.host_preparation.resolve",
        "workspace.lifecycle.begin",
        "workspace.lifecycle.prepare",
        "workspace.lifecycle.prepare_delivered",
        "workspace.lifecycle.renderer_unavailable",
    ];

    /// The closed credential and terminal input routes cannot carry values in JSON.
    const SECRET_MAIN: [&str; 7] = [
        "models.endpoint.configure_credential",
        "models.endpoint.create_with_credential",
        "models.endpoint.replace_credential",
        "tools.credential.create",
        "tools.credential.rotate",
        "terminal.user.sensitive.submit",
        "agent.mcp.secret.set",
    ];

    #[test]
    fn r03_registry_is_closed_and_secret_routes_remain_separate() {
        let mut keys = BTreeSet::new();
        for route in registry() {
            assert!(keys.insert((format!("{:?}", route.surface), route.method)));
            assert!(!route.request_type.is_empty() && !route.response_type.is_empty());
            if route.auth == RouteAuth::SecretInput {
                assert!(
                    matches!(route.owner, RouteOwner::Model | RouteOwner::Tool)
                        || (route.owner == RouteOwner::Agent
                            && matches!(
                                route.method,
                                "terminal.user.sensitive.submit" | "agent.mcp.secret.set"
                            )),
                    "{} is outside the closed secret input surface",
                    route.method
                );
                assert_ne!(route.idempotency, RouteIdempotency::ReadOnly);
            }
            #[cfg(not(feature = "p08-contract-driver"))]
            assert!(!route.test_only && !route.method.contains("e2e"));
        }

        // The renderer-reachable surface is exactly the audited set, module by
        // module: a route added to Main is a new capability for the renderer and has
        // to be accepted here on purpose. A stale count is how this inventory went
        // unnoticed before, so the per-module table is compared rather than the
        // total alone.
        let modules: [(&str, &[RouteDescriptor]); 19] = [
            ("media", media::ROUTES),
            ("browser", browser::ROUTES),
            ("chrome", chrome::ROUTES),
            ("memory", memory::ROUTES),
            ("workspace", workspace::ROUTES),
            ("git", git::ROUTES),
            ("package", package::ROUTES),
            ("security", security::ROUTES),
            ("model", model::ROUTES),
            ("agent", agent::ROUTES),
            ("agent_hooks", agent_hooks::ROUTES),
            ("agent_mcp", agent_mcp::ROUTES),
            ("agent_plugins", agent_plugins::ROUTES),
            ("agent_skills", agent_skills::ROUTES),
            ("tool", tool::ROUTES),
            ("runtime", runtime::ROUTES),
            ("terminal", terminal::ROUTES),
            ("terminal_input", terminal_input::ROUTES),
            ("ui", ui::ROUTES),
        ];
        for (name, routes) in modules {
            let expected = MAIN_PER_MODULE
                .iter()
                .find_map(|(module, count)| (*module == name).then_some(*count))
                .expect("every module is audited");
            let actual = routes.iter().filter(|r| r.surface == RouteSurface::Main).count();
            assert_eq!(
                actual, expected,
                "{name} changed the Main surface: audit the new route, its \
                 authentication class and its preload binding before updating the table"
            );
        }

        // Authentication classes do not leak across surfaces: a renderer entry is
        // either trusted-renderer or the single-use secret channel, never the
        // authenticated main class that Core commands use, and no test-only route is
        // reachable from the renderer at all.
        let main: Vec<&RouteDescriptor> =
            registry().filter(|r| r.surface == RouteSurface::Main).collect();
        assert_eq!(main.len(), MAIN_PER_MODULE.iter().map(|(_, n)| n).sum::<usize>());
        for route in &main {
            assert_ne!(
                route.auth,
                RouteAuth::AuthenticatedMain,
                "{} is renderer-reachable but authenticated as a Core command",
                route.method
            );
            assert!(
                !route.test_only,
                "{} is a test-only route on the renderer surface",
                route.method
            );
        }
        let secret: BTreeSet<&str> =
            main.iter().filter(|r| r.auth == RouteAuth::SecretInput).map(|r| r.method).collect();
        assert_eq!(secret, SECRET_MAIN.into_iter().collect::<BTreeSet<_>>());

        for (surface, auth) in [
            (RouteSurface::Main, RouteAuth::TrustedRenderer),
            (RouteSurface::Core, RouteAuth::AuthenticatedMain),
        ] {
            let activity_query = registry()
                .find(|route| route.surface == surface && route.method == "agent.activity.query")
                .expect("the activity query is exposed on both audited surfaces");
            assert_eq!(activity_query.owner, RouteOwner::Agent);
            assert_eq!(activity_query.auth, auth);
            assert_eq!(activity_query.idempotency, RouteIdempotency::ReadOnly);
            assert_eq!(activity_query.cancellation, RouteCancellation::None);
            assert_eq!(activity_query.request_type, "ActivityQueryRequestV1");
            assert_eq!(activity_query.response_type, "ActivityQueryResponseV1");
            assert!(!activity_query.test_only);
        }

        let main_methods: BTreeSet<&str> = main.iter().map(|r| r.method).collect();
        let core_methods: BTreeSet<&str> =
            registry().filter(|r| r.surface == RouteSurface::Core).map(|r| r.method).collect();
        let main_only: BTreeSet<&str> = main_methods.difference(&core_methods).copied().collect();
        let core_only: BTreeSet<&str> = core_methods.difference(&main_methods).copied().collect();
        assert_eq!(main_only, MAIN_ONLY.into_iter().collect::<BTreeSet<_>>());
        // CORE_ONLY deliberately does not list the contract-driver route: it only enters the registry
        // when `p08-contract-driver` is enabled. That feature is off for this crate's own test run but
        // ON for a workspace-wide `--all-features` run, so the expected set has to account for it in
        // both cases. The equality stays exact either way - any OTHER new Core-only route, and any
        // missing one, still fails this assertion.
        #[cfg(feature = "p08-contract-driver")]
        let expected_core_only: BTreeSet<&str> = CORE_ONLY
            .into_iter()
            .chain(["internal.e2e.p08_contract_driver.run_vertical_suite"])
            .collect();
        #[cfg(not(feature = "p08-contract-driver"))]
        let expected_core_only: BTreeSet<&str> = CORE_ONLY.into_iter().collect();
        assert_eq!(core_only, expected_core_only);

        assert!(core_route("__proto__").is_none());
        assert!(!side_effecting("runtime.readiness"));
        assert!(side_effecting("fs.write_file"));
        assert!(side_effecting("models.endpoint.configure_credential"));
    }
}
