use super::*;

fn found(script: &str) -> Vec<String> {
    of(
        script,
        Some(Path::new(r"C:\Dev\project\src")),
        &Area::workspace(PathBuf::from(r"C:\Dev\project")),
    )
    .outside
}

const COPY: &str = r"C:\Copies\project\k1";
const OWN_TEMP: &str = r"C:\Temp\aura-agents\ses_1";

/// A sub-agent in a worktree: its copy and its temporary folder are its own.
fn sub_agent() -> Area {
    let own = vec![PathBuf::from(COPY), PathBuf::from(OWN_TEMP)];
    Area { roots: own.clone(), deletable: own, temp: Some(PathBuf::from(OWN_TEMP)) }
}

/// Whether the script deletes, whether every deletion is in the sub-agent's own places, and the
/// places outside them it names.
fn deletions(script: &str) -> (bool, bool, Vec<String>) {
    let places = of(script, Some(Path::new(COPY)), &sub_agent());
    (places.deletes, places.deletions_allowed, places.outside)
}

#[test]
fn writes_outside_the_workspace_are_seen() {
    for (script, place) in [
        (r"Set-Content C:\Windows\x.txt 'a'", r"C:\Windows\x.txt"),
        (r"Add-Content -Path C:\Users\me\notes.txt -Value x", r"C:\Users\me\notes.txt"),
        (r"'x' | Out-File -FilePath D:\out.txt", r"D:\out.txt"),
        (r"Copy-Item .\a.txt C:\Temp\a.txt", r"C:\Temp\a.txt"),
        (
            r"Copy-Item -Path a.txt -Destination 'C:\Program Files (x86)\App'",
            r"C:\Program Files (x86)\App",
        ),
        (r"Move-Item a.txt ..\..\elsewhere", r"..\..\elsewhere"),
        (r"echo hi > C:\Temp\hi.txt", r"C:\Temp\hi.txt"),
        (r"Get-Date >> ..\..\log.txt", r"..\..\log.txt"),
        (r"dir 2>C:\err.txt", r"C:\err.txt"),
        (r"New-Item -ItemType Directory C:\NewFolder", r"C:\NewFolder"),
        (r"mkdir \\server\share\x", r"\\server\share\x"),
        (r"copy a.txt D:\backup\", r"D:\backup\"),
        (r"robocopy . E:\mirror /E", r"E:\mirror"),
        (r"Set-ItemProperty HKCU:\Software\X -Name Y -Value 1", r"HKCU:\Software\X"),
        (r"cd C:\Users; Set-Content notes.txt x", "notes.txt"),
        (r"Set-Location -Path D:\data; New-Item a.txt", "a.txt"),
        (r"[System.IO.File]::WriteAllText('C:\Temp\a.txt', 'x')", r"C:\Temp\a.txt"),
        (r"[IO.File]::Copy('a.txt', 'C:\Temp\b.txt')", r"C:\Temp\b.txt"),
        (r#"powershell -c "Set-Content C:\Temp\x.txt 1""#, r"C:\Temp\x.txt"),
        (r#"cmd /c "echo x > C:\Temp\y.txt""#, r"C:\Temp\y.txt"),
        (r"echo x>C:\Temp\z.txt", r"C:\Temp\z.txt"),
        (r#"echo x>"C:\Temp\q.txt""#, r"C:\Temp\q.txt"),
        (r"if ($ok) { Set-Content C:\Temp\w.txt x }", r"C:\Temp\w.txt"),
    ] {
        assert_eq!(found(script), vec![place.to_owned()], "{script}");
    }
}

#[test]
fn writes_inside_and_what_the_text_does_not_say_run_unasked() {
    for script in [
        "Set-Content notes.txt 'a'",
        r"Set-Content C:\Dev\project\notes.txt x",
        r"Copy-Item C:\Users\me\template.txt .\template.txt",
        r"Copy-Item ..\README.md docs\README.md",
        "echo hi > out.txt",
        "npm run build 2>&1",
        "dir > $null",
        "git status > nul",
        "Set-Content $path 'x'",
        r"Out-File -FilePath $env:NO_SUCH_VARIABLE_HERE\x.txt",
        r"Set-Content C:\Temp\*.txt x",
        r"Get-Content C:\Windows\win.ini",
        "Set-Item Env:FOO bar",
        r"cd C:\Users; Get-ChildItem",
        r"python build.py --out C:\dist",
        r"cd ..; Set-Content notes.txt x",
        r"Write-Output 'see > C:\Temp\x.txt'",
        // A here-string is data: what it says is written as text, not run.
        "@'\nSet-Content C:\\Temp\\x.txt 1 ; echo x > C:\\Temp\\y.txt\n'@ | Set-Content notes.md",
    ] {
        assert_eq!(found(script), Vec::<String>::new(), "{script}");
    }
}

#[test]
fn without_a_known_directory_only_absolute_places_are_seen() {
    let area = Area::workspace(PathBuf::from(r"C:\Dev\project"));
    assert_eq!(of(r"Set-Content C:\Temp\a.txt x", None, &area).outside, vec![r"C:\Temp\a.txt"]);
    assert_eq!(of(r"Set-Content ..\a.txt x", None, &area).outside, Vec::<String>::new());
}

#[test]
fn a_sub_agents_temporary_folder_is_its_own() {
    for script in [
        r#"'x' | Set-Content -Encoding utf8 "$env:TEMP\g1024-diff.js""#,
        r"Set-Content ${env:TMP}\a.txt x",
        r"echo x > %TEMP%\b.txt",
        r"New-Item -ItemType Directory -Force -Path $env:TEMP\shots",
    ] {
        assert_eq!(deletions(script), (false, true, vec![]), "{script}");
    }
    // A conversation without a folder of its own writes the real TEMP, outside its workspace.
    if std::env::var_os("TEMP").is_some() {
        assert_eq!(found(r"Set-Content $env:TEMP\a.txt x"), vec![r"$env:TEMP\a.txt"]);
    }
}

#[test]
fn deletions_that_name_places_in_the_area_are_known() {
    for script in [
        "Remove-Item dist -Recurse -Force",
        r"Remove-Item -LiteralPath .\a.txt, .\b.txt",
        r"rm -r -fo .\build",
        "del /q *.bak",
        "rd /s /q out",
        r"Remove-Item $env:TEMP\g1024-shots\*.png",
        "if (Test-Path x.txt) { Remove-Item x.txt -Force }",
        "cmd /c rd /s /q node_modules",
        r#"powershell -c "Remove-Item C:\Temp\aura-agents\ses_1\a.js""#,
        "git clean -fdx",
        "cd sub; Remove-Item old.txt",
        r"Remove-Item C:\Copies\project\k1\notes.txt",
    ] {
        assert_eq!(deletions(script), (true, true, vec![]), "{script}");
    }
}

#[test]
fn deletions_the_text_does_not_place_are_not_known() {
    for script in [
        "Remove-Item $png -Force",
        "Get-ChildItem *.tmp | Remove-Item",
        "$files | ForEach-Object { Remove-Item $_ }",
        "[IO.File]::Delete('a.txt')",
        "(Get-Item a.txt).Delete()",
        "powershell -EncodedCommand UgBlAG0A",
        r"Remove-Item C:\Copies\*\old",
        "Clear-RecycleBin -Force",
        "Write-Output (Remove-Item a.txt)",
        // A folder the text cannot follow leaves every later place unknown.
        "cd $dir; Remove-Item old.txt",
        "Set-Location -Path $x; Remove-Item a.txt",
        "pushd $env:NO_SUCH_AURA_FOLDER; del a.txt",
    ] {
        let (deletes, allowed, _) = deletions(script);
        assert!(deletes && !allowed, "{script}");
    }
}

/// Whether every deletion is one the sub-agent's own folders hold: its targets named there, or
/// left to a variable or the pipeline while the command stands there.
fn here(script: &str, directory: Option<&str>) -> bool {
    let places = of(script, directory.map(Path::new), &sub_agent());
    places.deletes && places.deletions_here
}

#[test]
fn deletions_left_to_a_variable_are_here_while_the_command_stands_in_its_own_folder() {
    for script in [
        "Remove-Item $png -Force",
        "Get-ChildItem *.tmp | Remove-Item",
        "$files | ForEach-Object { Remove-Item $_.FullName }",
        "Remove-Item dist -Recurse -Force",
        r"Remove-Item $env:TEMP\shots\*.png",
        "cd sub; Remove-Item $old",
        r#"powershell -c "Remove-Item $x""#,
        r"Remove-Item sub\*\cache -Recurse",
    ] {
        assert!(here(script, Some(COPY)), "{script}");
    }
}

#[test]
fn deletions_named_outside_or_not_read_are_not_here() {
    for script in [
        // Named outside its own folders.
        r"Remove-Item C:\Dev\project\a.txt",
        r"Remove-Item $png; Remove-Item ..\..\other -Recurse",
        r"Remove-Item C:\Copies\*\old",
        r"Remove-Item ..\*\cache",
        r"Remove-Item $env:USERPROFILE\Documents\a.txt",
        // Where the command stands is not known.
        "cd $dir; Remove-Item $old",
        "cd ..; Remove-Item $old",
        // Deletions the reader does not take apart.
        r"[IO.File]::Delete('C:\Windows\a.txt')",
        "(Get-Item a.txt).Delete()",
        "Clear-RecycleBin -Force",
        "Write-Output (Remove-Item $x)",
        "powershell -EncodedCommand UgBlAG0A",
    ] {
        assert!(!here(script, Some(COPY)), "{script}");
    }
    // A terminal's folder is not known from what is typed.
    assert!(!here("Remove-Item $png", None));
    assert!(here(r"Remove-Item C:\Copies\project\k1\a.txt", None));
}

#[test]
fn deletions_outside_the_area_are_written_outside() {
    assert_eq!(
        deletions(r"Remove-Item C:\Dev\project\a.txt"),
        (true, false, vec![r"C:\Dev\project\a.txt".to_owned()])
    );
    assert_eq!(
        deletions(r"Remove-Item ..\..\other -Recurse; Remove-Item mine.txt"),
        (true, false, vec![r"..\..\other".to_owned()])
    );
}

#[test]
fn a_main_agent_deletes_nowhere_unasked_and_writes_its_sub_agents_copies() {
    let area = Area {
        roots: vec![PathBuf::from(r"C:\Dev\project"), PathBuf::from(COPY)],
        deletable: vec![],
        temp: None,
    };
    let places = of("Remove-Item a.txt", Some(Path::new(r"C:\Dev\project")), &area);
    assert!(places.deletes && !places.deletions_allowed && places.outside.is_empty());
    let places = of(r"Remove-Item C:\Copies\project\k1\a.txt", None, &area);
    assert!(places.deletes && !places.deletions_allowed && places.outside.is_empty());
    assert!(of(r"Set-Content C:\Copies\project\k1\a.txt x", None, &area).outside.is_empty());
}
