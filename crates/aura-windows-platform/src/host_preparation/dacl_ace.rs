//! Compare DACL ACEs by type, flags, rights, object GUIDs and SID.
//! A SID allow-list is not an ACE identity.
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DaclAceIdentity {
    pub ace_type: String,
    pub flags: String,
    pub rights: String,
    pub object_guid: String,
    pub inherited_object_guid: String,
    pub sid: String,
}

/// Decode an SDDL rights field into its numeric mask.
///
/// Windows prints the same grant two ways: the generic spelling it was given
/// (GXGR) and the numeric mask the object reports (0x1200a9). Comparing the
/// text would call one grant two different grants, so the comparison works on
/// masks. None means this parser does not decode the token sequence, and the
/// caller then compares text — never silently equal.
///
/// The generic mappings are the file-object ones, which is also what the device
/// ACL editor applies: GR = 0x120089, GW = 0x120116, GX = 0x1200A0.
#[must_use]
pub fn rights_mask(rights: &str) -> Option<u32> {
    const TOKENS: [(&str, u32); 12] = [
        ("FA", 0x001F01FF),
        ("FR", 0x00120089),
        ("FW", 0x00120116),
        ("FX", 0x001200A0),
        ("GR", 0x00120089),
        ("GW", 0x00120116),
        ("GX", 0x001200A0),
        ("GA", 0x10000000),
        ("RC", 0x00020000),
        ("SD", 0x00010000),
        ("WD", 0x00040000),
        ("WO", 0x00080000),
    ];
    if let Some(hex) = rights.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).ok();
    }
    let mut mask = 0_u32;
    let mut rest = rights;
    while !rest.is_empty() {
        let (token, value) = TOKENS.iter().find(|(token, _)| rest.starts_with(token))?;
        mask |= value;
        rest = &rest[token.len()..];
    }
    (mask != 0).then_some(mask)
}

impl DaclAceIdentity {
    pub fn allow_mask(mask: u32, sid: &str) -> Self {
        Self {
            ace_type: "A".into(),
            flags: String::new(),
            rights: format!("0x{mask:x}"),
            object_guid: String::new(),
            inherited_object_guid: String::new(),
            sid: sid.into(),
        }
    }
}

pub fn parse_dacl_ace_identities(sddl: &str) -> io::Result<Vec<DaclAceIdentity>> {
    let body = sddl.strip_prefix("D:").ok_or_else(|| io::Error::other("maintenance.dacl_sddl"))?;
    let aces = strip_dacl_control(body)?;
    let mut identities = Vec::new();
    let mut rest = aces;
    while !rest.is_empty() {
        let inner =
            rest.strip_prefix('(').ok_or_else(|| io::Error::other("maintenance.dacl_ace"))?;
        let end = inner.find(')').ok_or_else(|| io::Error::other("maintenance.dacl_ace"))?;
        identities.push(parse_ace(&inner[..end])?);
        rest = &inner[end + 1..];
    }
    Ok(identities)
}

pub fn ensure_only_added_ace(
    before: &str,
    after: &str,
    expected: &DaclAceIdentity,
) -> io::Result<()> {
    let mut remaining = ace_counts(&parse_dacl_ace_identities(after)?);
    for (ace, count) in ace_counts(&parse_dacl_ace_identities(before)?) {
        if remaining.remove(&ace) != Some(count) {
            return Err(io::Error::other("maintenance.ace_identity_changed"));
        }
    }
    if remaining.len() == 1 && remaining.get(expected) == Some(&1) {
        return Ok(());
    }
    Err(io::Error::other("maintenance.ace_restore_not_exact"))
}

/// Whether current is exactly required, optionally plus one ACE that is
/// byte-for-byte the llowed identity.
///
/// Compared per complete ACE identity (type, flags, rights, object GUIDs, SID),
/// so a duplicate, a Deny, a changed mask, an inheritance flag, or any other
/// extra entry fails. A SID allow-list would not be enough: the same SID with a
/// wider mask is a different grant.
pub fn matches_with_allowed_extra(
    current: &str,
    required: &str,
    allowed: &DaclAceIdentity,
) -> io::Result<bool> {
    let mut current_counts = ace_counts(&parse_dacl_ace_identities(current)?);
    let required_counts = ace_counts(&parse_dacl_ace_identities(required)?);
    let allowed_count = current_counts.get(allowed).copied().unwrap_or(0);
    if allowed_count > 1 {
        // A duplicated capability ACE is not a legitimate coexistence.
        return Ok(false);
    }
    if allowed_count == 1 {
        current_counts.remove(allowed);
    }
    Ok(current_counts == required_counts)
}

/// Count ACEs by *canonical* identity, so GXGR and 0x1200a9 count as the
/// same grant while a different mask stays different.
///
/// This is a *multi-set* comparison and is only used where the question is "does
/// this exact ACE exist N times" — the planned capability delta. It deliberately
/// ignores order, so it must never decide whether two DACLs grant the same access;
/// use [`ordered_ace_sequences_match`] for that.
fn ace_counts(aces: &[DaclAceIdentity]) -> BTreeMap<DaclAceIdentity, usize> {
    let mut counts = BTreeMap::new();
    for ace in aces {
        let canonical = canonical_ace(ace);
        *counts.entry(canonical).or_insert(0) += 1;
    }
    counts
}

/// The ACE identities of a DACL rendering, **in file order**.
///
/// The only normalisation applied is the rights spelling: Windows prints the same
/// grant as `GXGR` or as `0x1200a9`, which is a representation difference and not a
/// permission difference. Every other field — type, flags, object GUIDs, SID — is
/// compared as reported, and the order is preserved exactly, because the order of
/// ACEs decides which entry Windows evaluates first.
pub fn ordered_ace_identities(sddl: &str) -> io::Result<Vec<DaclAceIdentity>> {
    Ok(parse_dacl_ace_identities(sddl)?.iter().map(canonical_ace).collect())
}

/// Whether two DACL renderings carry the same ACEs in the same order.
///
/// Order matters: the same Deny and Allow for one SID grant different access
/// depending on which comes first, so a reordered DACL is a different DACL.
pub fn ordered_ace_sequences_match(expected: &str, actual: &str) -> io::Result<bool> {
    Ok(ordered_ace_identities(expected)? == ordered_ace_identities(actual)?)
}

/// Which observation of one object a side of the comparison is.
///
/// The two sides are never interchangeable: the frozen original is what a recovery
/// has to recognise, the planned expectation is what the approved plan asked for,
/// and the read-back is what the object actually reports. Naming them at the call
/// site keeps "compare the plan" and "compare the object" from being confused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaclObservationV1 {
    /// The descriptor captured before Aura's change; the recovery reference.
    FrozenOriginal,
    /// What the approved plan says the object must look like afterwards.
    PlannedExpectation,
    /// What the object reports after the write.
    ActualReadBack,
}

/// Which targeted write is being judged.
///
/// Both W7 operations are the *same* explicit `SetFileSecurityW` DACL write, so both
/// can only ever lose the kernel-owned marker; no operation here re-establishes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaclWriteOperationV1 {
    /// Adds the planned non-inheritable compatibility ACEs to the drive root.
    ApplyPlannedAces,
    /// Removes those exact ACEs again.
    RevokePlannedAces,
}

/// What an expected/actual DACL comparison shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaclTransitionV1 {
    /// The DACL is identical, ACE order included.
    Unchanged,
    /// The only difference is that `SE_DACL_AUTO_INHERITED` is gone, which is the one
    /// transition the approved W7 rule allows for these two writes. The ordered ACE
    /// sequence and every other control bit are still exactly compared.
    ApprovedAutoInheritedMarker,
    /// Something else differs — a reordered ACE list, another control bit, a
    /// protection change — and it has to be reported as a real difference.
    Unapproved,
}

/// The control letters between `D:` and the first ACE, as Windows prints them.
pub fn dacl_control_letters(sddl: &str) -> io::Result<String> {
    dacl_control(sddl)
}

/// Compares what one targeted write was expected to produce with what the object
/// actually reports, and classifies the **control-field** difference.
///
/// This is deliberately separate from the ACE list: an apply adds ACEs and a revoke
/// removes them, so their ordered lists differ by exactly the planned delta — that is
/// checked with [`ordered_ace_sequences_match`] against the expected delta, never by
/// feeding two different lists to this function. What is judged here is only what may
/// move in the descriptor's control field: the kernel-owned auto-inherited marker, in
/// the one direction an explicit DACL write produces.
///
/// Scope: the DACL control field of the object the approved plan names. Owner, group
/// and the integrity label are not part of it — the caller captures and compares them,
/// because they are not DACL text.
#[must_use]
pub fn classify_control_transition(
    operation: DaclWriteOperationV1,
    expected_side: DaclObservationV1,
    actual_side: DaclObservationV1,
    expected_control: &str,
    actual_control: &str,
) -> DaclTransitionV1 {
    // The sides are not interchangeable, so a caller cannot accidentally compare two
    // plans or two read-backs and call the result a verified write.
    let sides_are_meaningful = actual_side == DaclObservationV1::ActualReadBack
        && match operation {
            DaclWriteOperationV1::ApplyPlannedAces => {
                expected_side == DaclObservationV1::PlannedExpectation
                    || expected_side == DaclObservationV1::FrozenOriginal
            }
            DaclWriteOperationV1::RevokePlannedAces => {
                expected_side != DaclObservationV1::ActualReadBack
            }
        };
    if !sides_are_meaningful {
        return DaclTransitionV1::Unapproved;
    }
    if expected_control == actual_control {
        return DaclTransitionV1::Unchanged;
    }
    // `AR` would mean the write asked Windows for an inheritance pass; `P`
    // (`SE_DACL_PROTECTED`) is the switch that blocks a parent's inheritable ACEs and
    // has to survive. Neither may move, and no other letter may appear or disappear.
    if expected_control.replace("AI", "") != actual_control.replace("AI", "")
        || expected_control.contains("AR")
        || actual_control.contains("AR")
    {
        return DaclTransitionV1::Unapproved;
    }
    match (expected_control.contains("AI"), actual_control.contains("AI")) {
        // An explicit DACL write is where the kernel drops the marker, and that is the
        // one transition this rule covers.
        (true, false) => DaclTransitionV1::ApprovedAutoInheritedMarker,
        // A targeted write cannot re-establish the marker, so a read-back that has it
        // where the reference did not is not the write the plan authorised.
        (false, true) => DaclTransitionV1::Unapproved,
        // Unreachable: equal control fields returned above.
        (true, true) | (false, false) => DaclTransitionV1::Unapproved,
    }
}

/// The rights field in its numeric spelling; every other field is untouched.
fn canonical_ace(ace: &DaclAceIdentity) -> DaclAceIdentity {
    match rights_mask(&ace.rights) {
        Some(mask) => DaclAceIdentity { rights: format!("0x{mask:x}"), ..ace.clone() },
        None => ace.clone(),
    }
}

/// The control letters between `D:` and the first ACE.
fn dacl_control(sddl: &str) -> io::Result<String> {
    let body = sddl.strip_prefix("D:").ok_or_else(|| io::Error::other("maintenance.dacl_sddl"))?;
    let end = body.find('(').unwrap_or(body.len());
    Ok(body[..end].to_owned())
}

fn strip_dacl_control(body: &str) -> io::Result<&str> {
    let mut index = 0;
    while index < body.len() {
        if body[index..].starts_with('P') {
            index += 1;
            continue;
        }
        if body[index..].starts_with("AR") || body[index..].starts_with("AI") {
            index += 2;
            continue;
        }
        break;
    }
    if index < body.len() && !body[index..].starts_with('(') && !body[index..].is_empty() {
        return Err(io::Error::other("maintenance.dacl_control"));
    }
    Ok(&body[index..])
}

fn parse_ace(inner: &str) -> io::Result<DaclAceIdentity> {
    let parts: Vec<&str> = inner.split(';').collect();
    if parts.len() != 6 || parts[0].is_empty() || parts[5].is_empty() {
        return Err(io::Error::other("maintenance.dacl_ace"));
    }
    Ok(DaclAceIdentity {
        ace_type: parts[0].into(),
        flags: parts[1].into(),
        rights: parts[2].into(),
        object_guid: parts[3].into(),
        inherited_object_guid: parts[4].into(),
        sid: parts[5].into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aura() -> DaclAceIdentity {
        DaclAceIdentity::allow_mask(0x12019f, "S-1-5-21-1-2-3-4")
    }

    #[test]
    fn restore_compares_complete_ace_identity_not_new_sid_names() {
        let before = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;0x1201bf;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";
        let aura_ace = "(A;;0x12019f;;;S-1-5-21-1-2-3-4)";
        let restored = format!("{before}{aura_ace}");
        ensure_only_added_ace(before, &restored, &aura()).expect("exact aura ace");
        let inherit = format!("{before}(A;OI;0x12019f;;;S-1-5-21-1-2-3-4)");
        assert!(ensure_only_added_ace(before, &inherit, &aura()).is_err());
        let deny = format!("{before}(D;;0x12019f;;;S-1-5-21-1-2-3-4)");
        assert!(ensure_only_added_ace(before, &deny, &aura()).is_err());
        let wrong_mask = format!("{before}(A;;0x1201bf;;;S-1-5-21-1-2-3-4)");
        assert!(ensure_only_added_ace(before, &wrong_mask, &aura()).is_err());
        let extra = format!("{restored}(A;;GR;;;WD)");
        assert!(ensure_only_added_ace(before, &extra, &aura()).is_err());
        let ac_mask_changed =
            "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;GR;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";
        assert!(
            ensure_only_added_ace(before, &format!("{ac_mask_changed}{aura_ace}"), &aura())
                .is_err()
        );
    }

    #[test]
    fn one_allowed_capability_ace_may_coexist_but_nothing_else_may() {
        let base = "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)\
(A;;0x1201bf;;;AC)(A;;0x1201bf;;;S-1-15-2-2)";
        let aura = aura();
        // Absent, and present exactly once: both are the required state.
        assert!(matches_with_allowed_extra(base, base, &aura).expect("parse"));
        let with_aura = format!("{base}(A;;0x12019f;;;S-1-5-21-1-2-3-4)");
        assert!(matches_with_allowed_extra(&with_aura, base, &aura).expect("parse"));
        // Duplicate, wrong mask, deny, inherited, other SID, other rights: no.
        let twice = format!("{with_aura}(A;;0x12019f;;;S-1-5-21-1-2-3-4)");
        assert!(!matches_with_allowed_extra(&twice, base, &aura).expect("parse"));
        let widened = format!("{base}(A;;0x1201bf;;;S-1-5-21-1-2-3-4)");
        assert!(!matches_with_allowed_extra(&widened, base, &aura).expect("parse"));
        let denied = format!("{base}(D;;0x12019f;;;S-1-5-21-1-2-3-4)");
        assert!(!matches_with_allowed_extra(&denied, base, &aura).expect("parse"));
        let inherited = format!("{base}(A;OI;0x12019f;;;S-1-5-21-1-2-3-4)");
        assert!(!matches_with_allowed_extra(&inherited, base, &aura).expect("parse"));
        let other_sid = format!("{base}(A;;0x12019f;;;S-1-5-21-9-9-9-9)");
        assert!(!matches_with_allowed_extra(&other_sid, base, &aura).expect("parse"));
        let unknown_extra = format!("{with_aura}(A;;GR;;;WD)");
        assert!(!matches_with_allowed_extra(&unknown_extra, base, &aura).expect("parse"));
        // A missing requirement is still missing.
        let without_ac = with_aura.replace("(A;;0x1201bf;;;AC)", "");
        assert!(!matches_with_allowed_extra(&without_ac, base, &aura).expect("parse"));
    }

    #[test]
    fn the_approved_marker_transition_never_excuses_an_ordered_or_real_difference() {
        // Two separate questions, judged separately: what the control field may do, and
        // whether the ACE sequence is the planned one. Neither excuses the other.
        let aces = "(A;;GRGWGX;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;RC)(A;;GRGWGX;;;AC)\
(A;;GRGWGX;;;S-1-15-2-2)";
        let marker = |operation, expected_side, expected: &str, actual: &str| {
            classify_control_transition(
                operation,
                expected_side,
                DaclObservationV1::ActualReadBack,
                expected,
                actual,
            )
        };

        // The measured apply and the measured targeted revoke both lose the marker.
        assert_eq!(
            marker(
                DaclWriteOperationV1::ApplyPlannedAces,
                DaclObservationV1::PlannedExpectation,
                "PAI",
                "P"
            ),
            DaclTransitionV1::ApprovedAutoInheritedMarker
        );
        assert_eq!(
            marker(
                DaclWriteOperationV1::RevokePlannedAces,
                DaclObservationV1::FrozenOriginal,
                "PAI",
                "P"
            ),
            DaclTransitionV1::ApprovedAutoInheritedMarker,
            "the targeted revoke is the same explicit write, so it loses the marker too"
        );
        assert_eq!(
            marker(
                DaclWriteOperationV1::ApplyPlannedAces,
                DaclObservationV1::PlannedExpectation,
                "P",
                "P"
            ),
            DaclTransitionV1::Unchanged
        );
        // A targeted write can never re-establish the marker.
        assert_eq!(
            marker(
                DaclWriteOperationV1::ApplyPlannedAces,
                DaclObservationV1::PlannedExpectation,
                "P",
                "PAI"
            ),
            DaclTransitionV1::Unapproved,
            "gaining the marker is not the authorised transition"
        );
        // Protection and other letters are real differences, wherever they move.
        for (expected, actual) in [("AI", "PAI"), ("PARI", "PAI"), ("P", "PAR"), ("", "P")] {
            assert_eq!(
                marker(
                    DaclWriteOperationV1::ApplyPlannedAces,
                    DaclObservationV1::PlannedExpectation,
                    expected,
                    actual
                ),
                DaclTransitionV1::Unapproved,
                "must stay unapproved: {expected} -> {actual}"
            );
        }
        // The sides are part of the rule: a caller may not compare two plans or two
        // read-backs and call it a verified write.
        assert_eq!(
            marker(
                DaclWriteOperationV1::ApplyPlannedAces,
                DaclObservationV1::FrozenOriginal,
                "P",
                "PAI"
            ),
            DaclTransitionV1::Unapproved
        );
        assert_eq!(
            classify_control_transition(
                DaclWriteOperationV1::ApplyPlannedAces,
                DaclObservationV1::PlannedExpectation,
                DaclObservationV1::PlannedExpectation,
                "P",
                "P"
            ),
            DaclTransitionV1::Unapproved
        );

        // The ACE sequence: the same ACEs in a different order are a different DACL,
        // because Windows evaluates the first match.
        let expected_dacl = format!("D:PAI{aces}");
        let swapped = "(A;;FA;;;SY)(A;;GRGWGX;;;WD)(A;;FA;;;BA)(A;;GRGX;;;RC)(A;;GRGWGX;;;AC)\
(A;;GRGWGX;;;S-1-15-2-2)";
        assert!(
            !ordered_ace_sequences_match(&expected_dacl, &format!("D:PAI{swapped}"))
                .expect("parse"),
            "a swapped ACE list must not compare equal"
        );
        // For one SID: Deny before Allow grants nothing, Allow before Deny grants the
        // allow. Order is the whole difference.
        assert!(
            !ordered_ace_sequences_match(
                "D:PAI(D;;GRGWGX;;;WD)(A;;GRGWGX;;;WD)(A;;FA;;;SY)",
                "D:P(A;;GRGWGX;;;WD)(D;;GRGWGX;;;WD)(A;;FA;;;SY)"
            )
            .expect("parse")
        );
        // A changed mask, an extra ACE or a missing ACE: real differences too.
        for actual in [
            format!("D:PAI{}", aces.replace("(A;;GRGX;;;RC)", "(A;;GR;;;RC)")),
            format!("D:PAI{aces}(A;;GR;;;WD)"),
            format!("D:PAI{}", aces.replace("(A;;GRGWGX;;;AC)", "")),
        ] {
            assert!(
                !ordered_ace_sequences_match(&expected_dacl, &actual).expect("parse"),
                "must not compare equal: {actual}"
            );
        }
    }

    #[test]
    fn other_resources_get_no_exception_at_all() {
        // The exception belongs to the W7 drive-root compatibility write. A temporary
        // object gets the same verdict as any other drift: the marker transition is
        // classified identically wherever it is read, and an inheritance-flag drift is
        // never excused on any object.
        let aces = "(A;OICIID;FA;;;SY)(A;OICIID;0x1200a9;;;BA)";
        assert_eq!(
            classify_control_transition(
                DaclWriteOperationV1::ApplyPlannedAces,
                DaclObservationV1::PlannedExpectation,
                DaclObservationV1::ActualReadBack,
                &dacl_control_letters(&format!("D:AI{aces}")).expect("control"),
                &dacl_control_letters(&format!("D:{aces}")).expect("control"),
            ),
            DaclTransitionV1::ApprovedAutoInheritedMarker
        );
        assert!(
            !ordered_ace_sequences_match(
                &format!("D:AI{aces}"),
                &format!("D:{}", aces.replace("(A;OICIID;FA;;;SY)", "(A;OICI;FA;;;SY)"))
            )
            .expect("parse"),
            "an inheritance-flag drift is never excused, on any object"
        );
    }

    #[test]
    fn parse_skips_protected_flags_and_keeps_inherit() {
        let aces =
            parse_dacl_ace_identities("D:P(A;OICI;FA;;;SY)(A;;0x1200a9;;;RC)").expect("parse");
        assert_eq!(aces[0].ace_type, "A");
        assert_eq!(aces[0].flags, "OICI");
        assert_eq!(aces[0].rights, "FA");
        assert_eq!(aces[1].sid, "RC");
        assert!(parse_dacl_ace_identities("O:BAG:SYD:(A;;FA;;;SY)").is_err());
    }
}
