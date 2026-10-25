//! The one reusable, session-scoped consent in the V2 authorization model.
//!
//! Every other approval is exact-operation: the user confirms one immutable
//! Operation fingerprint, and a new call with a different script, correlation or
//! permission snapshot is a new Operation that has to be confirmed again. A
//! Native session consent is the confirmed product exception — the owner
//! approved "this session may keep running ordinary PowerShell/CMD through the
//! Native backend until it is revoked, permissions tighten, or Core restarts" —
//! so its scope is deliberately allowed to cover later Operations.
//!
//! The exception is narrow on purpose and every narrowing is enforced in code:
//!
//! * only the trusted built-in Shell tools qualify ([`NativeShell`]),
//! * the consent names the Core runtime instance that issued it, so a restarted
//!   Core never inherits it,
//! * the consent names the session permission revision it was approved under, so
//!   a later tightening cannot be undone by returning to FullAccess,
//! * the consent records the principal that asked, so another session or a
//!   different actor in the same session cannot borrow it,
//! * each new call still gets its own full Operation fingerprint, its own
//!   preparation plan, its own short-lived decision and its own dispatch
//!   binding ([`crate::NativeSessionConsentRef`] is the proof that the new
//!   fingerprint descends from an approved consent).
//!
//! This module owns only the shared vocabulary. Policy decides when the consent
//! applies, the Store decides whether it is still live, and the Final Dispatch
//! Guard decides whether the derived decision may reach a Backend.

use aura_domain_common::PrincipalId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The built-in Shell tools a Native session consent may cover.
///
/// The variant is the conservative identity: a consent is approved for one
/// concrete shell that is part of the signed-in product, never for "some
/// process on this machine". It is the *tool that was approved*, which is an audit
/// fact; the *range the approval covers* is [`NativeSessionConsentScope::shells`].
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NativeShell {
    /// The wire spelling is the product's own tool name for this shell, not a
    /// mechanical case conversion, so a reader of the contract sees the same word
    /// the approval dialog does.
    #[serde(rename = "powershell")]
    PowerShell,
    Cmd,
}

impl NativeShell {
    /// Every trusted built-in Shell, in a stable order.
    ///
    /// A session approval answers "may this conversation keep running ordinary
    /// Shell commands through Native", and PowerShell and CMD are two spellings of
    /// that one question rather than two privileges. The list is the range one
    /// approval covers; the approval still records the Shell it was given for.
    pub const ALL: [Self; 2] = [Self::PowerShell, Self::Cmd];

    #[must_use]
    pub const fn action_kind(self) -> &'static str {
        match self {
            Self::PowerShell => "shell.powershell",
            Self::Cmd => "shell.cmd",
        }
    }

    /// The canonical Shell whose canonical tool name is `action_kind`.
    #[must_use]
    pub fn from_action_kind(action_kind: &str) -> Option<Self> {
        match action_kind {
            "shell.powershell" => Some(Self::PowerShell),
            "shell.cmd" => Some(Self::Cmd),
            // Terminal tools answer the same session-wide run question. The
            // canonical representative preserves the existing two-shell scope;
            // legacy single-shell consents are never widened.
            "terminal.open" | "terminal.write" => Some(Self::PowerShell),
            _ => None,
        }
    }

    /// The bounded ASCII key stored and compared by the Store. It is a column
    /// value, not a user-visible name.
    #[must_use]
    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::PowerShell => "powershell",
            Self::Cmd => "cmd",
        }
    }
}

/// The actor a Native session consent is bound to, and the range it covers.
///
/// Naming the principal is what keeps "the same conversation" from meaning "the
/// same browser tab": an approval granted while the user was driving the session
/// does not authorize an Agent-initiated call in that session, and no other
/// session or Core runtime instance can borrow it. It is an authorization
/// binding, not an operating-system isolation claim — a Native process still
/// runs as the signed-in Windows user and is not confined to the workspace.
///
/// `shells` is the covered range. It is stored as a set rather than derived from a
/// single Shell so that two facts stay separate: *which
/// tool was approved* (`approved_shell`, audit) and *what the approval covers*
/// (`shells`, authorization). Nothing in the reuse test reads `approved_shell`.
///
/// Two stored shapes are read, and they are not interchangeable:
///
/// * the current shape — `approved_shell` plus the `shells` range,
/// * the predecessor shape — a single `shell`, whose approval covered exactly that
///   one Shell. It is read as a one-Shell range and **never widened**: covering a
///   second Shell is a new answer from the user, not something a reader may infer
///   from the fact that both Shells are built in today.
#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize)]
#[schemars(deny_unknown_fields)]
pub struct NativeSessionConsentScope {
    pub principal_id: PrincipalId,
    /// The Shell the user actually confirmed. Audit only.
    pub approved_shell: NativeShell,
    /// The trusted built-in Shells this consent authorizes.
    pub shells: std::collections::BTreeSet<NativeShell>,
}

/// The stored field shapes this scope is read from.
///
/// A stored row is read strictly: an unknown field, a mixture of the two shapes, or a
/// record naming no approved Shell at all is refused rather than resolved by
/// preference. Refusing here means the row cannot authorize anything, which is the
/// fail-closed direction; silently picking one of two conflicting readings is not.
///
/// The wire type is deliberately private and has no `JsonSchema`: the *documented*
/// contract is the current shape, and a predecessor field that exists only so old rows
/// stay readable should not read as something a new writer may produce.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSessionConsentScopeWire {
    principal_id: PrincipalId,
    #[serde(default)]
    approved_shell: Option<NativeShell>,
    #[serde(default)]
    shells: Option<std::collections::BTreeSet<NativeShell>>,
    /// The predecessor field. See [`NativeSessionConsentScope`].
    #[serde(default)]
    shell: Option<NativeShell>,
}

impl TryFrom<NativeSessionConsentScopeWire> for NativeSessionConsentScope {
    type Error = &'static str;

    fn try_from(wire: NativeSessionConsentScopeWire) -> Result<Self, Self::Error> {
        match (wire.approved_shell, wire.shell, wire.shells) {
            (Some(approved_shell), None, Some(shells)) => {
                Ok(Self { principal_id: wire.principal_id, approved_shell, shells })
            }
            (None, Some(shell), None) => Ok(Self {
                principal_id: wire.principal_id,
                approved_shell: shell,
                shells: std::iter::once(shell).collect(),
            }),
            (None, None, _) => Err("native session consent names no approved shell"),
            (Some(_), None, None) => Err("native session consent has no covered range"),
            (Some(_), Some(_), _) | (None, Some(_), Some(_)) => {
                Err("native session consent mixes two stored shapes")
            }
        }
    }
}

impl<'de> Deserialize<'de> for NativeSessionConsentScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = NativeSessionConsentScopeWire::deserialize(deserializer)?;
        Self::try_from(wire).map_err(serde::de::Error::custom)
    }
}

impl NativeSessionConsentScope {
    /// The scope one session approval of `approved` creates.
    #[must_use]
    pub fn for_approved_shell(principal_id: PrincipalId, approved: NativeShell) -> Self {
        Self {
            principal_id,
            approved_shell: approved,
            shells: NativeShell::ALL.into_iter().collect(),
        }
    }

    /// Whether this consent covers `shell`.
    #[must_use]
    pub fn covers(&self, shell: NativeShell) -> bool {
        self.shells.contains(&shell)
    }
}

/// The consent scope an Operation may be approved into, if any.
///
/// Both the approval flow and the later reuse test call this one function, so an
/// approval can never issue a consent that no later call could use, and a later
/// call can never be refused for a reason the approval did not know about.
///
/// A reusable consent is only available to a trusted built-in Shell tool run by
/// the signed-in user. MCP servers, Plugins, Skills, Hooks, other Sessions and
/// Agent-initiated calls in the same Session keep the exact-operation rule.
#[must_use]
pub fn native_session_consent_scope(
    source: &crate::SecuritySourceRef,
    action_kind: &str,
    principal_id: PrincipalId,
) -> Option<NativeSessionConsentScope> {
    if !matches!(source, crate::SecuritySourceRef::BuiltInTool { .. }) {
        return None;
    }
    let shell = NativeShell::from_action_kind(action_kind)?;
    Some(NativeSessionConsentScope::for_approved_shell(principal_id, shell))
}

/// Whether a session approval of this backend may mint a reusable consent.
///
/// A reusable consent is a statement about the *Native* execution path: "this
/// conversation may keep running ordinary Shell commands as the signed-in Windows
/// user without asking again". That is the range the user is shown when they are
/// asked to accept an execution with reduced isolation. A session approval of a
/// restricted backend is a different answer to a different question — it covers the
/// operation's own class of work under the mode ceiling, and it is not the reduction
/// the consent describes.
///
/// Requiring the backend class here rather than inferring it from the tool name is
/// what keeps a restricted `AllowSession` from being recorded as a Native consent
/// that no dispatch would then honour.
#[must_use]
pub fn backend_class_can_hold_a_session_consent(class: crate::BackendClass) -> bool {
    matches!(class, crate::BackendClass::Native)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shell_approval_covers_both_built_in_shells_and_records_which_was_approved() {
        let principal = PrincipalId::new();
        let powershell = native_session_consent_scope(
            &crate::SecuritySourceRef::BuiltInTool {
                identity_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"shell"),
            },
            "shell.powershell",
            principal,
        )
        .expect("a built-in shell may be approved");
        assert_eq!(powershell.approved_shell, NativeShell::PowerShell);
        assert!(powershell.covers(NativeShell::PowerShell));
        assert!(powershell.covers(NativeShell::Cmd));

        let cmd = native_session_consent_scope(
            &crate::SecuritySourceRef::BuiltInTool {
                identity_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"shell"),
            },
            "shell.cmd",
            principal,
        )
        .expect("the other built-in shell may be approved");
        assert_eq!(cmd.approved_shell, NativeShell::Cmd);
        assert_eq!(
            cmd.shells, powershell.shells,
            "the covered range does not depend on which shell was approved first"
        );
    }

    /// The stored predecessor shape, in the wire spelling both Shells actually used.
    ///
    /// A row written before the range existed records one `shell`, and the approval it
    /// records covered exactly that Shell. Reading it must not fail — an inert old row
    /// that cannot be decoded poisons every query that touches the table — and must not
    /// widen it either: PowerShell is not covered by a CMD approval just because the
    /// current product can approve both.
    #[test]
    fn a_stored_single_shell_consent_stays_readable_and_narrow() {
        for (spelling, shell) in
            [("cmd", NativeShell::Cmd), ("powershell", NativeShell::PowerShell)]
        {
            let stored = serde_json::json!({
                "principal_id": PrincipalId::new(),
                "shell": spelling,
            });
            let scope = serde_json::from_value::<NativeSessionConsentScope>(stored).unwrap_or_else(
                |error| panic!("the predecessor {spelling} row must be readable: {error}"),
            );
            assert_eq!(scope.approved_shell, shell, "the approved Shell survives the read");
            assert_eq!(
                scope.shells,
                std::iter::once(shell).collect(),
                "the predecessor approval covered exactly one Shell"
            );
            assert!(scope.covers(shell));
            for other in NativeShell::ALL.into_iter().filter(|candidate| *candidate != shell) {
                assert!(
                    !scope.covers(other),
                    "an old {spelling} approval must not widen to {other:?}"
                );
            }
        }
    }

    #[test]
    fn a_mixed_or_unknown_stored_scope_is_refused_rather_than_resolved() {
        let principal = PrincipalId::new();
        for (label, stored) in [
            (
                "both shapes at once",
                serde_json::json!({
                    "principal_id": principal,
                    "shell": "cmd",
                    "approved_shell": "powershell",
                    "shells": ["powershell", "cmd"],
                }),
            ),
            (
                "a predecessor shell beside a range",
                serde_json::json!({
                    "principal_id": principal,
                    "shell": "cmd",
                    "shells": ["cmd"],
                }),
            ),
            (
                "no approved shell at all",
                serde_json::json!({ "principal_id": principal, "shells": ["cmd"] }),
            ),
            (
                "an unknown field",
                serde_json::json!({
                    "principal_id": principal,
                    "approved_shell": "cmd",
                    "shells": ["cmd"],
                    "extra": true,
                }),
            ),
        ] {
            assert!(
                serde_json::from_value::<NativeSessionConsentScope>(stored).is_err(),
                "{label} must be refused"
            );
        }
    }

    #[test]
    fn no_other_source_can_be_approved_into_a_consent() {
        for source in [
            crate::SecuritySourceRef::McpServer { server_id: "s".into() },
            crate::SecuritySourceRef::Plugin { plugin_id: "p".into() },
            crate::SecuritySourceRef::Skill { skill_id: "k".into() },
            crate::SecuritySourceRef::Hook { hook_id: "h".into() },
            crate::SecuritySourceRef::UserInterface,
        ] {
            assert!(
                native_session_consent_scope(&source, "shell.powershell", PrincipalId::new())
                    .is_none(),
                "{source:?} must not be approvable into a reusable consent"
            );
        }
    }
}
