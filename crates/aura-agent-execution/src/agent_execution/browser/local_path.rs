//! A file `browser.open` is given by `path`: relative to the workspace (or to a sub-agent's working
//! copy), or written in full when it lies inside that root.

use std::path::{Component, Path, Prefix};

/// Where a `path` written for `browser.open` points, against the root it is opened in.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Given {
    /// Not written in full: read relative to the root, as before.
    Relative,
    /// Written in full and inside the root: the part below it, with `/` between names.
    Inside(String),
    /// Written in full and not inside the root, or through `.` or `..`.
    Outside,
}

pub(super) fn given(root: &Path, path: &str) -> Given {
    if !Path::new(path).is_absolute() {
        return Given::Relative;
    }
    let (Some((root_volume, root_names)), Some((volume, names))) =
        (parts(root), parts(Path::new(path)))
    else {
        return Given::Outside;
    };
    let inside = volume == root_volume
        && names.len() > root_names.len()
        && root_names.iter().zip(&names).all(|(a, b)| a.to_lowercase() == b.to_lowercase());
    if inside { Given::Inside(names[root_names.len()..].join("/")) } else { Given::Outside }
}

/// The volume (`d:` or `//server/share`) and the folder and file names of a path written in full.
/// Windows writes the same volume with or without the `\\?\` prefix, and names letters of any case.
fn parts(path: &Path) -> Option<(String, Vec<String>)> {
    let mut components = path.components();
    let volume = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                format!("{}:", char::from(letter).to_ascii_lowercase())
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                "//{}/{}",
                server.to_string_lossy().to_lowercase(),
                share.to_string_lossy().to_lowercase()
            ),
            _ => return None,
        },
        _ => return None,
    };
    let mut names = Vec::new();
    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => names.push(name.to_string_lossy().into_owned()),
            // A path that steps through `.` or `..` is not taken apart here.
            _ => return None,
        }
    }
    Some((volume, names))
}

#[cfg(test)]
mod tests {
    use super::{Given, given};
    use std::path::Path;

    #[test]
    fn a_file_written_in_full_inside_the_root_is_read_below_it() {
        let root = Path::new(r"\\?\D:\Dev\Run\workspace");
        assert_eq!(
            given(root, "D:/Dev/Run/workspace/web/local-page.html"),
            Given::Inside("web/local-page.html".into())
        );
        assert_eq!(
            given(root, r"d:\dev\run\WORKSPACE\web\page.html"),
            Given::Inside("web/page.html".into())
        );
        assert_eq!(given(Path::new(r"D:\ws"), r"\\?\D:\ws\a.html"), Given::Inside("a.html".into()));
    }

    #[test]
    fn a_relative_path_is_left_as_it_was_written() {
        let root = Path::new(r"D:\ws");
        for path in ["web/index.html", "index.html", "../index.html", "D:index.html"] {
            assert_eq!(given(root, path), Given::Relative, "{path}");
        }
    }

    #[test]
    fn a_file_written_in_full_outside_the_root_is_outside() {
        let root = Path::new(r"D:\Dev\ws");
        for path in [
            r"D:\Dev\other\index.html",
            r"D:\Dev\ws-old\index.html",
            r"C:\Dev\ws\index.html",
            r"D:\Dev\ws",
            r"D:\Dev\ws\..\other\index.html",
            r"\\server\share\Dev\ws\index.html",
        ] {
            assert_eq!(given(root, path), Given::Outside, "{path}");
        }
    }
}
