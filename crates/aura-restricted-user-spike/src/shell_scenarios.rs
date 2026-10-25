//! Core command behavior through the real launcher. No language project builds.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

pub const CASES: [&str; 6] =
    ["streams", "nonzero", "script_error", "cancel", "output_limit", "timeout"];

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
            Ok(observation) => json!({"case":case,"status":"passed","observation":observation}),
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
    let args = if shell == "powershell" {
        aura_backend_windows_user::environment::powershell_command(powershell_script(case)?)
    } else {
        let work = root.join("allowed");
        fs::write(work.join("r06-scenario.cmd"), cmd_script(case)?.replace('\n', "\r\n"))?;
        fs::write(
            work.join("r06-wait.ps1"),
            format!(
                "{}\n[Console]::WriteLine('child_ready'); [Console]::Out.Flush(); [Threading.Thread]::Sleep(30000)",
                aura_backend_windows_user::environment::POWERSHELL_BOOTSTRAP
            ),
        )?;
        ["/d", "/q", "/u", "/e:on", "/v:off", "/c", "r06-scenario.cmd"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    let mut process = crate::monitor::launch(sid, executable, &args, root, device, Some(runtime))?;
    if case == "streams" {
        process.write_stdin(b"r06-input\r\n")?;
    }
    let observation = crate::monitor::collect_until(
        &mut process,
        case == "cancel",
        16_384,
        Duration::from_secs(if case == "timeout" { 3 } else { 10 }),
    )?;
    validate(case, &observation)?;
    Ok(observation)
}

fn powershell_script(case: &str) -> Result<&'static str> {
    Ok(match case {
        "streams" => {
            "$inputText=[Console]::In.ReadLine(); [Console]::Error.WriteLine('r06-stderr'); @{input_ok=($inputText -eq 'r06-input'); shell='powershell'} | ConvertTo-Json -Compress; exit 0"
        }
        "nonzero" => "exit 23",
        "script_error" => "throw 'r06-script-error'",
        "cancel" => {
            "& (Join-Path $PSHOME 'pwsh.exe') -MTA -NoLogo -NoProfile -NonInteractive -Command '[Console]::WriteLine(''child_ready''); [Console]::Out.Flush(); [Threading.Thread]::Sleep(30000)'"
        }
        "output_limit" => {
            "while($true){[Console]::Out.Write('x'*4096); [Console]::Out.Flush(); [Console]::Error.Write('e'*4096)}"
        }
        "timeout" => "[Threading.Thread]::Sleep(30000)",
        _ => anyhow::bail!("unknown fixed shell case"),
    })
}

fn cmd_script(case: &str) -> Result<&'static str> {
    Ok(match case {
        "streams" => {
            "@echo off\nset /p R06_INPUT=\nif not \"%R06_INPUT%\"==\"r06-input\" exit /b 81\n>&2 echo r06-stderr\necho {\"input_ok\":true,\"shell\":\"cmd\"}\nexit /b 0\n"
        }
        "nonzero" => "@exit /b 23\n",
        "script_error" => {
            "@echo off\ncall r06-deliberately-absent-command.cmd\nexit /b %errorlevel%\n"
        }
        "cancel" => "@echo off\npwsh -MTA -NoLogo -NoProfile -NonInteractive -File r06-wait.ps1\n",
        "output_limit" => {
            "@echo off\n:more\necho xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n>&2 echo eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\ngoto more\n"
        }
        "timeout" => "@echo off\npwsh -MTA -NoLogo -NoProfile -NonInteractive -File r06-wait.ps1\n",
        _ => anyhow::bail!("unknown fixed shell case"),
    })
}

fn validate(case: &str, observation: &Value) -> Result<()> {
    ensure!(observation["tree_empty"] == true, "shell tree remains: {observation}");
    let expected = match case {
        "cancel" => "cancelled",
        "output_limit" => "output_limit",
        "timeout" => "timeout",
        _ => "exited",
    };
    ensure!(observation["termination"] == expected, "wrong shell termination: {observation}");
    match case {
        "streams" => ensure!(
            observation["exit_code"] == 0
                && observation["payload"]["input_ok"] == true
                && observation["stderr_excerpt"]
                    .as_str()
                    .is_some_and(|text| text.contains("r06-stderr")),
            "streams differ: {observation}"
        ),
        "nonzero" => ensure!(observation["exit_code"] == 23, "exit code lost: {observation}"),
        "script_error" => ensure!(
            observation["exit_code"].as_i64().is_some_and(|code| code != 0)
                && observation["stderr_bytes"].as_u64().is_some_and(|bytes| bytes > 0),
            "script error hidden: {observation}"
        ),
        _ => (),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_results_keep_failure_timeout_and_cleanup_distinct() {
        let nonzero = json!({"termination":"exited","tree_empty":true,"exit_code":23});
        assert!(validate("nonzero", &nonzero).is_ok());
        assert!(validate("timeout", &nonzero).is_err());
        assert!(validate("script_error", &nonzero).is_err());
        let mut cancelled = json!({"termination":"cancelled","tree_empty":true});
        assert!(validate("cancel", &cancelled).is_ok());
        cancelled["tree_empty"] = json!(false);
        assert!(validate("cancel", &cancelled).is_err());
    }
}
