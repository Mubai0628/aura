//! What a session mode authorizes on its own, before any explicit approval.
//!
//! # The mode is a default, not a ceiling
//!
//! The three modes are **default permissions**: they say which file capabilities a
//! session may exercise with no approval. They are deliberately not an upper bound on
//! what the user can approve — the confirmed product semantics are that any default
//! denial is approvable. A ReadOnly session can approve writing one named target, and a
//! WorkspaceReadWrite session can approve writing one ordinary directory outside its
//! workspace, without changing the whole session mode.
//!
//! So this module answers exactly one question: *does the mode allow this operation
//! without asking?* It never answers "is this operation impossible?". The approval
//! flow in [`crate::policy`] is what decides whether an operation the mode does not
//! cover may proceed, and it is reached for every such operation from every entry.
//!
//! # What remains absolute
//!
//! Two things are not a matter of degree and are still refused here, because no
//! approval can express them:
//!
//! * a child agent acting where only its parent's authority was established, and
//! * Provider/tool configuration mutation by a model — that is the user's own control
//!   plane, and a model may not use configuration write to grant itself a different
//!   authority. The trusted settings and maintenance entries keep their own separate,
//!   authenticated authority; this function is about Agent tool calls only.
//!
//! Network stays mode-sensitive in the same "default" sense: the lower two modes never
//! reach the network on their own, and reaching it is an approval the policy asks for.
use aura_security_contract::{InvocationActor, PermissionMode, ResolvedCapability as C};

/// Whether the session mode alone authorizes this operation, without an approval.
///
/// `false` means "this needs the user's explicit approval", not "this is refused".
/// The caller must still route the operation through the approval flow, which is
/// where an unapproved operation is denied.
pub fn permits(
    actor: InvocationActor,
    mode: PermissionMode,
    capabilities: &[C],
    network: bool,
    controlled_web: bool,
) -> bool {
    // A non-Agent caller (the authenticated Main/UI route family, the user's own
    // command) has its own authenticated authority rather than a session mode.
    if !matches!(actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent) {
        return true;
    }
    // Provider/tool configuration is the user's control plane. This is the one
    // capability no approval of an ordinary Agent operation may confer, so it is
    // refused rather than merely "asked about".
    if capabilities.contains(&C::ConfigurationWrite) {
        return false;
    }
    if mode == PermissionMode::FullAccess {
        return true;
    }
    if network && (mode == PermissionMode::ReadOnly || !controlled_web) {
        return false;
    }
    capabilities.iter().all(|c| match c {
        C::WorkspaceFileRead
        | C::MemoryRead
        | C::GitRead
        | C::ConfigurationRead
        | C::ToolRegistryRead
        | C::ToolEphemeralExposureWrite
        | C::AgentTaskRead
        | C::AgentTaskWrite
        | C::InteractiveTerminal
        | C::BackgroundProcess => true,
        C::ProcessExecute => mode != PermissionMode::ReadOnly,
        // A trusted MCP server's tool; ReadOnly asks once per server per turn.
        C::ExternalExtensionCall => mode != PermissionMode::ReadOnly,
        // A workspace write, delete or move is what WorkspaceReadWrite and FullAccess
        // already authorize without asking. In ReadOnly it is not authorized *by the
        // mode* — the approval flow asks, and one approved target may proceed.
        C::WorkspaceFileWrite
        | C::WorkspaceFileCreate
        | C::WorkspaceFileDelete
        | C::WorkspaceFileMove
        | C::GitWrite => mode == PermissionMode::WorkspaceReadWrite,
        C::MemoryWrite => mode == PermissionMode::WorkspaceReadWrite,
        // An **external** object is never covered by a session default, in any mode below
        // FullAccess. The workspace defaults are about the workspace: WorkspaceReadWrite
        // authorizes reading inside the workspace, not reading "anything reachable because a
        // read capability was declared". So an ordinary external object asks in the two lower
        // modes, and FullAccess is the one mode whose confirmed default reaches ordinary
        // external objects (handled by the early `FullAccess => true` above). Asking is not a
        // refusal: the answer is a precise approval for the object and capability named.
        C::ExternalFileRead
        | C::ExternalFileWrite
        | C::ExternalFileCreate
        | C::ExternalFileDelete => false,
        C::OrdinaryNetwork | C::PublicNetworkRead => {
            controlled_web && mode == PermissionMode::WorkspaceReadWrite
        }
        _ => false,
    })
}

pub(crate) fn process(c: C) -> bool {
    matches!(c, C::ProcessExecute | C::InteractiveTerminal | C::BackgroundProcess)
}

/// Whether an operation outside the session mode is refused rather than asked about.
///
/// Returning `false` from [`permits`] normally means "ask the user". Two cases are not
/// approvable, so they stay refusals and the approval flow is never reached for them:
///
/// * **Configuration write by a model.** Provider/tool configuration is the user's own
///   control plane, reachable only through the trusted settings and maintenance entries.
///   An ordinary Agent file approval must not become a route to rewriting the authority
///   the agent runs under.
/// * **A child agent acting where only its parent's authority was established.** The
///   parent's approval is not the child's; a child that finds itself outside the mode's
///   default has no authority of its own to fall back on, and the parent's session grant
///   is checked separately by the grant matching in [`crate::policy`].
///
/// Everything else the mode does not cover — including every ordinary file capability —
/// is deliberately *not* here: it asks.
pub(crate) fn refuses_outright(actor: InvocationActor, capabilities: &[C]) -> bool {
    if capabilities.contains(&C::ConfigurationWrite) {
        return true;
    }
    matches!(actor, InvocationActor::ChildAgent)
}

pub(crate) fn ordinary_network(c: C) -> bool {
    matches!(
        c,
        C::OrdinaryNetwork
            | C::PublicNetworkRead
            | C::PrivateNetworkAccess
            | C::DataEgress
            | C::GitRemoteRead
            | C::GitRemoteWrite
            | C::DownloadArtifact
            | C::ExternalServiceRead
            | C::ExternalServiceMutation
            | C::BrowserRead
            | C::BrowserInteraction
    )
}
