//! Which model-named external paths must keep asking for approval.
//!
//! FullAccess follows the existing policy for an *ordinary* external file: the user
//! confirmed that no per-directory grant has to be created first. That confirmation is
//! about ordinary objects. A path that lands in an operating-system location, or in one of
//! the user's credential stores, is not an ordinary file, and the policy already treats
//! those resources as always-worth-asking. Classification has to say which one a path is,
//! so the descriptor the operation carries is the real category rather than "outside the
//! workspace, therefore ordinary".
//!
//! The input is the path the platform verified — its final path after links were followed
//! — so a link that points into a protected location is classified as the location it
//! really reaches. Metadata verification compares object identity and native names
//! against the roots the environment declares; it does not read file content.

mod namespace;
use std::path::{Path, PathBuf};

/// How a verified external path is classified for the approval chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExternalPathClass {
    /// An ordinary file the existing policy decides by mode.
    Ordinary,
    /// An operating-system location: always asks, in every mode.
    System,
    /// One of the user's own credential or key stores: always asks.
    SensitiveUser,
}

/// Classifies a verified external path.
pub(super) fn classify(final_path: &Path) -> ExternalPathClass {
    let candidate = fold(final_path);
    if sensitive_roots().iter().any(|root| is_within(&candidate, root)) {
        return ExternalPathClass::SensitiveUser;
    }
    if system_roots().iter().any(|root| is_within(&candidate, root)) {
        return ExternalPathClass::System;
    }
    ExternalPathClass::Ordinary
}

/// Whether a directory grant rooted at `root` may cover `candidate` by its recursive scope.
///
/// # Why a recursive grant is not enough on its own
///
/// A directory approval is a recursive scope record: the user approved the directory and, with
/// it, the ordinary descendants that exist now and those created later. That is the confirmed
/// rule and it is what makes the approval usable without re-asking per level.
///
/// "Ordinary descendants" is the load-bearing part. The protected ranges are not ordinary
/// objects that happen to sit under a directory — they are a *different kind of authority*
/// (the machine's own locations, and the user's credential stores), which the confirmed
/// decision says must be approved by their own capability. So a grant on `C:\Users\me` must not
/// silently authorize `C:\Users\me\.ssh`, and a grant on a project directory must not silently
/// authorize a credential store someone linked inside it. Without this check the recursive
/// default of the parent would mask the protected subrange, which is exactly the masking the
/// decision forbids.
///
/// # What it does and does not decide
///
/// It answers one question: *does this candidate fall in a protected range the root's own
/// classification did not already put it in?* It is deliberately asked of the **final** path,
/// after links are followed, so a junction that merely sits inside the root but points into a
/// protected location is classified as the location it really reaches — the same rule the
/// direct (non-grant) path already uses.
///
/// It does not weaken the grant. A protected candidate is neither allowed nor denied here: it
/// is routed to the protected-resource descriptors, where it is decided by its own capability
/// and can still be approved once or for the session, exactly like a protected path reached
/// without a grant. Approving a subdirectory also never widens the parent: this is a
/// carve-out, never a promotion.
pub(super) fn escapes_recursive_grant(root: &Path, candidate: &Path) -> bool {
    // An ordinary candidate under a root that is itself ordinary is covered by the grant: this
    // is the common case and it must stay cheap and permissive.
    let candidate_class = classify(candidate);
    if candidate_class == ExternalPathClass::Ordinary {
        return false;
    }
    // The candidate is in a protected range. It is only a *carve-out* if the root the user
    // approved was not already inside that same protected range: approving `C:\Windows\System32`
    // directly is an approval of a system location by its own capability, and its ordinary
    // children must not each become a fresh carve-out.
    candidate_class != classify(root)
}

/// The operating-system locations a tool must not treat as ordinary files.
///
/// The roots come from the environment rather than from hard-coded drive letters, so a
/// system installed elsewhere is still recognized. A missing variable contributes no root.
fn system_roots() -> Vec<String> {
    let mut roots = Vec::new();
    for name in ["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
        if let Some(value) = std::env::var_os(name) {
            roots.push(fold(Path::new(&value)));
        }
    }
    roots
}

/// The user's own credential and key stores.
///
/// These are the locations whose contents are secrets by construction: an agent that can
/// read them can impersonate the user elsewhere. The list is deliberately about stores,
/// not about every dotfile: it names what holds keys, tokens or saved credentials.
fn sensitive_roots() -> Vec<String> {
    let mut roots = Vec::new();
    let Some(profile) = std::env::var_os("USERPROFILE") else {
        return roots;
    };
    let profile = PathBuf::from(profile);
    for relative in [
        ".ssh",
        ".aws",
        ".azure",
        ".gnupg",
        ".kube",
        ".docker\\config.json",
        ".netrc",
        "_netrc",
        "AppData\\Local\\Microsoft\\Credentials",
        "AppData\\Roaming\\Microsoft\\Credentials",
        "AppData\\Roaming\\Microsoft\\Crypto",
        "AppData\\Local\\Google\\Chrome\\User Data",
        "AppData\\Roaming\\Mozilla\\Firefox\\Profiles",
    ] {
        roots.push(fold(&profile.join(relative)));
    }
    roots
}

/// Normalize transport syntax only. Namespace comparison verifies object identity
/// or uses the actual parent's native name rules, including sensitive directories.
fn fold(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    strip_verbatim_prefix(&text)
}

/// The path text with a leading extended-length prefix removed, when it names a drive path.
///
/// `\\?\C:\dir` and `\\?\UNC\server\share` are the two extended forms. The first is the one
/// the platform returns for an ordinary local directory; the second is a UNC share and is
/// returned as `\\?\UNC\server\share`, which corresponds to `\\server\share` — so the `UNC`
/// marker is replaced by the two leading separators it stands for rather than dropped.
pub(crate) fn strip_verbatim_prefix(text: &str) -> String {
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return text.to_owned();
    };
    // Only a drive-rooted remainder is the same object as its short spelling. Anything else
    // (a device path, a volume GUID path) is a different namespace and is left alone.
    let mut characters = rest.chars();
    match (characters.next(), characters.next()) {
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic() => rest.to_owned(),
        _ => text.to_owned(),
    }
}

/// True when `candidate` is `root` itself or sits below it.
///
/// The boundary check keeps `C:\Windows-old` from matching the `C:\Windows` root: a
/// prefix is only a containment when it ends at a separator.
fn is_within(candidate: &str, root: &str) -> bool {
    namespace::within(candidate, root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real protected directory the environment declares, with a synthetic name below
    /// it: the classification is exercised without reading anything.
    #[test]
    fn an_operating_system_location_is_a_system_path() {
        let Ok(system_root) = std::env::var("SystemRoot") else {
            return;
        };
        let inside = Path::new(&system_root).join("System32").join("drivers").join("etc");
        assert_eq!(classify(&inside.join("hosts")), ExternalPathClass::System);
        // The root itself and its direct children are inside it too.
        assert_eq!(classify(Path::new(&system_root)), ExternalPathClass::System);
        assert_eq!(
            classify(&Path::new(&system_root).join("review-synthetic.txt")),
            ExternalPathClass::System
        );
        // A sibling whose name merely starts with the root's text is not inside it.
        let sibling = format!("{system_root}-review-synthetic");
        assert_eq!(classify(Path::new(&sibling)), ExternalPathClass::Ordinary);
        // Neither the separator nor the case decides the answer.
        let spelled = format!("{system_root}/SYSTEM32/Review-Synthetic");
        assert_eq!(classify(Path::new(&spelled)), ExternalPathClass::System);
    }

    #[test]
    fn one_of_the_users_own_stores_is_sensitive() {
        let Ok(profile) = std::env::var("USERPROFILE") else {
            return;
        };
        let profile = Path::new(&profile);
        for relative in [
            ".ssh\\review-synthetic-key",
            ".aws\\review-synthetic-credentials",
            "AppData\\Local\\Microsoft\\Credentials\\review-synthetic",
            "AppData\\Roaming\\Microsoft\\Crypto\\review-synthetic",
        ] {
            assert_eq!(
                classify(&profile.join(relative)),
                ExternalPathClass::SensitiveUser,
                "{relative}"
            );
        }
        // A name that merely starts with the store's text is a different object.
        assert_eq!(
            classify(&profile.join(".ssh-review-synthetic").join("key")),
            ExternalPathClass::Ordinary
        );
    }

    #[test]
    fn an_ordinary_location_stays_ordinary() {
        let temporary = std::env::temp_dir().join("aura-review-synthetic.txt");
        assert_eq!(classify(&temporary), ExternalPathClass::Ordinary);
        assert_eq!(classify(Path::new("relative.txt")), ExternalPathClass::Ordinary);
    }

    /// The platform's own final path names the same object as its short spelling.
    ///
    /// This is the counter-example behind the review's F6: the native API returns an ordinary
    /// local directory in the extended (`\\?\C:\Windows`) form, and comparing that spelling
    /// against the environment roots classified a genuine system directory as *ordinary*. The
    /// fix is to remove a transport detail of the native API, not to special-case one string, so
    /// both spellings must classify identically — and a path that merely contains the characters
    /// must not be rewritten.
    #[test]
    fn the_native_final_path_spelling_names_the_same_object() {
        let Ok(system_root) = std::env::var("SystemRoot") else {
            return;
        };
        let inside = Path::new(&system_root).join("System32");
        let verbatim = PathBuf::from(format!(r"\\?\{}\System32", system_root));
        assert_eq!(
            fold(&inside),
            fold(&verbatim),
            "the extended-length prefix is a spelling of the same object"
        );
        assert_eq!(classify(&verbatim), classify(&inside));
        assert_eq!(classify(&verbatim), ExternalPathClass::System);
        // The UNC extended form maps to the share spelling it really names.
        assert_eq!(fold(Path::new(r"\\?\UNC\server\share\dir")), r"\\server\share\dir");
        // A volume-GUID or device path is a *different* namespace, so it is left alone rather
        // than rewritten into something it does not mean.
        let device = r"\\?\Volume{00000000-0000-0000-0000-000000000000}\dir";
        assert_eq!(fold(Path::new(device)), device);
        // A name that merely contains the characters is not a prefix at all.
        assert_eq!(fold(Path::new(r"C:\review\?\folder")), r"C:\review\?\folder");
    }

    /// A recursive grant still covers ordinary descendants — including deep and not-yet-created
    /// ones — so the carve-out is a narrow exception and not a general re-approval per level.
    #[test]
    fn ordinary_descendants_stay_covered_at_any_depth() {
        let Ok(temporary) = std::env::var("TEMP") else {
            return;
        };
        let approved = Path::new(&temporary).join("aura-review-synthetic-project");
        for relative in ["src", "src\\nested", "src\\nested\\deeper", "future-child"] {
            assert!(
                !escapes_recursive_grant(&approved, &approved.join(relative)),
                "{relative} is an ordinary descendant and stays covered"
            );
        }
        // A target that does not exist yet is still an ordinary descendant by its parent.
        assert!(!escapes_recursive_grant(
            &approved,
            &approved.join("created-later").join("file.txt")
        ));
    }

    /// A recursive grant on a parent does not silently cover a protected subrange.
    ///
    /// This is the counter-example the confirmed decision asks for: the parent's recursive
    /// default is what makes an approval usable, and it must not be the thing that masks a
    /// protected subdirectory. The subrange is neither allowed nor refused here — it is
    /// routed to a decision of its own — so this asserts the routing, not an outcome.
    #[test]
    fn a_recursive_grant_does_not_mask_a_protected_subrange() {
        let Ok(profile) = std::env::var("USERPROFILE") else {
            return;
        };
        let profile = Path::new(&profile);
        // A grant on the user's own profile must not authorize the credential store inside it.
        assert!(
            escapes_recursive_grant(profile, &profile.join(".ssh").join("id_ed25519")),
            "a credential store under an approved parent needs its own approval"
        );
        assert!(
            escapes_recursive_grant(profile, &profile.join(".aws").join("credentials")),
            "every credential store is carved out, not just the first"
        );
        // An ordinary file under the same parent is still covered by the parent's grant: the
        // carve-out is narrow and must not turn every descendant into a fresh question.
        assert!(
            !escapes_recursive_grant(
                profile,
                &profile.join("review-synthetic-project").join("src")
            ),
            "an ordinary descendant stays covered by the recursive grant"
        );
        // The carve-out is asked of the final path, so the root's own classification decides
        // whether the candidate is a carve-out at all: approving a protected location directly
        // is an approval of that location, and its own children are not each re-carved.
        let store = profile.join(".ssh");
        assert!(
            !escapes_recursive_grant(&store, &store.join("id_ed25519")),
            "a grant rooted inside one protected location does not re-carve its own children"
        );
    }

    /// The carve-out never promotes: it cannot make a protected path approvable by itself, and
    /// a system location under an approved ordinary directory is likewise carved out.
    #[test]
    fn a_protected_subrange_under_an_ordinary_root_is_carved_out() {
        let Ok(system_root) = std::env::var("SystemRoot") else {
            return;
        };
        let Ok(temporary) = std::env::var("TEMP") else {
            return;
        };
        let ordinary = Path::new(&temporary).join("aura-review-synthetic-project");
        assert!(
            escapes_recursive_grant(&ordinary, &Path::new(&system_root).join("System32")),
            "an operating-system location is not an ordinary descendant"
        );
        assert!(
            !escapes_recursive_grant(&ordinary, &ordinary.join("src").join("main.rs")),
            "an ordinary file below the approved root stays covered"
        );
    }
}
