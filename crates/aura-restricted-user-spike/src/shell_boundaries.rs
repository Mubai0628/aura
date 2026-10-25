//! Fixed shell argument/path probes for the shared Home/Pro package.
//! Long paths exercise explicit extended paths, not arbitrary third-party CLI support.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub const CASES: [&str; 3] = ["unicode_path", "arguments_pipeline", "extended_path"];

pub fn run(
    shell: &str,
    executable: &Path,
    runtime: &Path,
    sid: &str,
    root: &Path,
    device: Option<&str>,
) -> Vec<Value> {
    CASES
        .into_iter()
        .map(|case| match execute(shell, case, executable, runtime, sid, root, device) {
            Ok(value) => json!({"case":case,"status":"passed","observation":value}),
            Err(error) => json!({"case":case,"status":"failed","error":error.to_string()}),
        })
        .collect()
}

fn execute(
    shell: &str,
    case: &str,
    executable: &Path,
    runtime: &Path,
    sid: &str,
    root: &Path,
    device: Option<&str>,
) -> Result<Value> {
    let work = root.join("allowed");
    let directory = work.join(format!("boundary-{shell}-{case}-中文 space"));
    fs::create_dir(&directory)?;
    let args = if shell == "powershell" {
        aura_backend_windows_user::environment::powershell_command(&powershell(case))
    } else {
        ensure!(shell == "cmd", "unknown boundary shell");
        let script = cmd(case);
        // ASCII source; the FOR variable carries the actual Unicode directory name.
        fs::write(work.join("r06-boundary.cmd"), script.replace('\n', "\r\n"))?;
        fs::write(
            work.join("r06-arguments-child.cmd"),
            concat!(
                "@echo off\r\n",
                "if not \"%~1\"==\"one two\" exit /b 71\r\n",
                "if not \"%~2\"==\"a&b\" exit /b 72\r\n",
                "if not \"%~3\"==\"literal!bang\" exit /b 73\r\n",
                "exit /b 0\r\n",
            ),
        )?;
        ["/d", "/q", "/u", "/e:on", "/v:off", "/c", "r06-boundary.cmd"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    let long = directory.join("segment0123456789".repeat(10)).join("segment0123456789".repeat(10));
    if case == "extended_path" {
        ensure!(long.as_os_str().len() > 300, "long-path fixture too short");
        fs::create_dir_all(&long)?;
    }
    let mut process = crate::monitor::launch(sid, executable, &args, root, device, Some(runtime))?;
    let observed = crate::monitor::collect(&mut process, false, 8192)?;
    ensure!(
        observed["tree_empty"] == true
            && observed["termination"] == "exited"
            && observed["exit_code"] == 0
            && observed["payload"]["boundary_ok"] == true,
        "shell boundary failed: {observed}"
    );
    if case != "arguments_pipeline" {
        let target = if case == "extended_path" { long } else { directory };
        let bytes = fs::read(target.join("value.txt"))?;
        let expected = if shell == "cmd" {
            "boundary-data\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>()
        } else {
            b"boundary-data".to_vec()
        };
        ensure!(bytes == expected, "shell wrote different bytes or wrong path");
    }
    Ok(observed)
}

fn powershell(case: &str) -> String {
    let find = format!(
        "$dir=@(Get-ChildItem -LiteralPath . -Directory -Filter 'boundary-powershell-{case}-*'); if($dir.Count -ne 1){{throw 'directory selection'}}; $path=$dir[0].FullName;\n"
    );
    let body = match case {
        "arguments_pipeline" => concat!(
            "$values=@('one two','a&b','literal!bang','quoted\"value');\n",
            "function Test-Args { param([string[]]$Values) if(($Values -join '|') -cne 'one two|a&b|literal!bang|quoted\"value'){throw 'arguments changed'} };\n",
            "Test-Args -Values $values;\n",
            "$result=1..4 | Where-Object {$_ % 2 -eq 0} | ForEach-Object {$_ * 3};\n",
            "if(($result -join ',') -ne '6,12'){throw 'pipeline changed'};\n",
        ).to_owned(),
        "extended_path" => format!(
            "$path='\\\\?\\'+[IO.Path]::Combine($path,'{0}','{0}','value.txt'); [IO.File]::WriteAllText($path,'boundary-data',[Text.UTF8Encoding]::new($false));\n",
            "segment0123456789".repeat(10)),
        _ => "$path=Join-Path $path 'value.txt'; [IO.File]::WriteAllText($path,'boundary-data',[Text.UTF8Encoding]::new($false));\n".into(),
    };
    format!("{find}{body}@{{boundary_ok=$true}}|ConvertTo-Json -Compress")
}

fn cmd(case: &str) -> String {
    let body = match case {
        "arguments_pipeline" => concat!(
            "call r06-arguments-child.cmd \"one two\" \"a&b\" \"literal!bang\"\n",
            "if errorlevel 1 exit /b 74\n",
            "echo pipeline-marker|findstr /x pipeline-marker > nul\n",
            "if errorlevel 1 exit /b 75\n",
        ).to_owned(),
        "extended_path" => format!(
            "for /d %%d in (\"boundary-cmd-extended_path-*\") do echo boundary-data>\"\\\\?\\%%~fd\\{0}\\{0}\\value.txt\"\nif errorlevel 1 exit /b 76\n",
            "segment0123456789".repeat(10)),
        _ => "for /d %%d in (\"boundary-cmd-unicode_path-*\") do echo boundary-data>\"%%~fd\\value.txt\"\nif errorlevel 1 exit /b 77\n".into(),
    };
    format!("@echo off\n{body}echo {{\"boundary_ok\":true}}\nexit /b 0\n")
}
