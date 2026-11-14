//! rustc's dependency files (`deps/<unit>.d`): one Makefile rule per output, then an empty rule
//! for every source the unit read. Workspace sources are relative to the workspace root, the
//! folder cargo runs rustc in; registry and generated sources are absolute.
use std::path::{Path, PathBuf};

/// The sources a dependency file lists, resolved against the workspace root.
pub(super) fn sources(text: &str, root: &Path) -> Vec<PathBuf> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.strip_suffix(':'))
        .filter(|path| !path.is_empty())
        .map(|path| {
            let path = PathBuf::from(path.replace("\\ ", " "));
            if path.is_absolute() { path } else { root.join(path) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn the_empty_rules_name_the_sources_and_relative_ones_start_at_the_root() {
        let text = "D:\\w\\target\\debug\\deps\\a-1.d: crates\\a\\src\\lib.rs crates\\a\\src\\b.rs\n\
                    \n\
                    D:\\w\\target\\debug\\deps\\liba-1.rlib: crates\\a\\src\\lib.rs crates\\a\\src\\b.rs\n\
                    \n\
                    crates\\a\\src\\lib.rs:\n\
                    crates\\a\\src\\b.rs:\n\
                    C:\\Users\\u\\.cargo\\registry\\src\\x\\lib.rs:\n\
                    D:\\w\\My\\ Files\\gen.rs:\n\
                    \n\
                    # env-dep:CARGO_PKG_NAME=a\n";
        let sources = super::sources(text, Path::new(r"D:\w"));
        assert_eq!(
            sources,
            [
                PathBuf::from(r"D:\w\crates\a\src\lib.rs"),
                PathBuf::from(r"D:\w\crates\a\src\b.rs"),
                PathBuf::from(r"C:\Users\u\.cargo\registry\src\x\lib.rs"),
                PathBuf::from(r"D:\w\My Files\gen.rs"),
            ]
        );
    }
}
