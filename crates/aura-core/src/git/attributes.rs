use super::*;
use aura_workspace_domain::{GitChangeV1, GitFileStatV1};
use std::collections::BTreeSet;
pub(super) const RAW_COMPARISON: &str = "未运行外部程序，按原始内容比较";
impl GitReader {
    pub(super) fn annotate_changes(&self, entries: &mut [GitChangeV1]) -> Result<()> {
        let selected = self.raw_comparison_paths(entries.iter().flat_map(|entry| {
            std::iter::once(entry.path.as_str()).chain(entry.original_path.as_deref())
        }))?;
        for entry in entries {
            if selected.contains(&entry.path)
                || entry.original_path.as_ref().is_some_and(|old| selected.contains(old))
            {
                entry.comparison_note = Some(RAW_COMPARISON.into());
            }
        }
        Ok(())
    }
    pub(super) fn annotate_stats(&self, files: &mut [GitFileStatV1]) -> Result<()> {
        let selected = self.raw_comparison_paths(files.iter().flat_map(|entry| {
            std::iter::once(entry.path.as_str()).chain(entry.original_path.as_deref())
        }))?;
        for entry in files {
            if selected.contains(&entry.path)
                || entry.original_path.as_ref().is_some_and(|old| selected.contains(old))
            {
                entry.comparison_note = Some(RAW_COMPARISON.into());
            }
        }
        Ok(())
    }
    fn raw_comparison_paths<'a>(
        &self,
        paths: impl Iterator<Item = &'a str>,
    ) -> Result<BTreeSet<String>> {
        let mut selected = BTreeSet::new();
        if self.programs.filters.is_empty() && self.programs.diffs.is_empty() {
            return Ok(selected);
        }
        let paths = paths.collect::<BTreeSet<_>>();
        let mut args =
            vec!["check-attr".into(), "-z".into(), "filter".into(), "diff".into(), "--".into()];
        let mut units = 64;
        for path in paths {
            let next = path.encode_utf16().count() + 4;
            ensure!(next < 24_000, "git.attribute_path_too_long");
            if units + next > 24_000 {
                self.collect_attributes(&args, &mut selected)?;
                args.truncate(5);
                units = 64;
            }
            args.push(path.into());
            units += next;
        }
        if args.len() > 5 {
            self.collect_attributes(&args, &mut selected)?;
        }
        Ok(selected)
    }
    fn collect_attributes(&self, args: &[String], selected: &mut BTreeSet<String>) -> Result<()> {
        // check-attr -z predates 2.40; no shell, converter or index write is involved.
        let bytes = self.command_owned(args.to_vec())?;
        let mut fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
        if fields.last() == Some(&&b""[..]) {
            fields.pop();
        }
        ensure!(fields.len() % 3 == 0, "git.attribute_format");
        for row in fields.chunks_exact(3) {
            let path = std::str::from_utf8(row[0])?;
            let attr = std::str::from_utf8(row[1])?;
            let driver = std::str::from_utf8(row[2])?;
            if (attr == "filter" && self.programs.filters.contains(driver))
                || (attr == "diff" && self.programs.diffs.contains(driver))
            {
                selected.insert(path.into());
            }
        }
        Ok(())
    }
}
