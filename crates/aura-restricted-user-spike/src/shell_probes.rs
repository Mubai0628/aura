//! Fixed core-shell probes. No user-language discovery, builds or installation.
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const SCOPE: &str = "r06-core-shells-v2";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolImage {
    name: String,
    path: PathBuf,
    sha256: String,
}

pub fn run(images: &[ToolImage], sid: &str, root: &Path, device_install_id: Option<&str>) -> Value {
    let candidates = images.iter().filter(|v| v.name == "powershell").collect::<Vec<_>>();
    let runtime = (candidates.len() == 1).then(|| candidates[0].path.parent()).flatten();
    let results = ["powershell", "cmd"].into_iter().map(|name| {
        let selected = images.iter().filter(|v| v.name == name).collect::<Vec<_>>();
        let result = if let (Some(runtime), [image]) = (runtime, selected.as_slice()) {
            probe(image, runtime, sid, root, device_install_id)
                .unwrap_or_else(|e| json!({"status":"failed","error":e.to_string()}))
        } else {
            json!({"status":"not_available","reason":"one reviewed shell and managed runtime required"})
        };
        json!({"tool":name,"result":result})
    }).collect::<Vec<_>>();
    json!({
        "schema":SCOPE,
        "environment_revision":aura_backend_windows_user::environment::REVISION,
        "results":results,
        "scope":"core command execution only; no language project builds; old toolchain reports do not satisfy this scope"
    })
}

fn probe(
    image: &ToolImage,
    runtime: &Path,
    sid: &str,
    root: &Path,
    device_install_id: Option<&str>,
) -> Result<Value> {
    ensure!(image.path.is_absolute(), "reviewed absolute image required");
    let digest = format!("{:x}", Sha256::digest(fs::read(&image.path)?));
    ensure!(digest.eq_ignore_ascii_case(&image.sha256), "reviewed image changed");
    let arguments = if image.name == "powershell" {
        fs::write(
            root.join("allowed/r06-powershell-child.ps1"),
            format!(
                "{}\n{}",
                aura_backend_windows_user::environment::POWERSHELL_BOOTSTRAP,
                include_str!("shell_fixtures/powershell-child.ps1")
            ),
        )?;
        powershell_arguments()
    } else {
        ensure!(
            image.path == aura_windows_platform::windows_directory()?.join("System32/cmd.exe"),
            "CMD must be bound to the Windows system component"
        );
        prepare_cmd(root)?
    };
    let mut process = crate::monitor::launch(
        sid,
        &image.path,
        &arguments,
        root,
        device_install_id,
        Some(runtime),
    )?;
    let execution = crate::monitor::collect(&mut process, false, 65_536)?;
    let validation = if image.name == "powershell" {
        validate_powershell(&execution, root)
    } else {
        validate_cmd(&execution, root, runtime)
    };
    if let Err(error) = validation {
        return Ok(
            json!({"status":"failed","error":error.to_string(),"image_sha256":digest,"execution":execution}),
        );
    }
    let scenarios = crate::shell_scenarios::run(
        &image.name,
        &image.path,
        runtime,
        sid,
        root,
        device_install_id,
    );
    let boundaries = crate::shell_boundaries::run(
        &image.name,
        &image.path,
        runtime,
        sid,
        root,
        device_install_id,
    );
    let passed = scenarios.iter().chain(&boundaries).all(|case| case["status"] == "passed");
    Ok(
        json!({"status":if passed {"passed"} else {"failed"},"image_sha256":digest,"execution":execution,"scenarios":scenarios,"boundaries":boundaries}),
    )
}

fn prepare_cmd(root: &Path) -> Result<Vec<String>> {
    let work = root.join("allowed");
    fs::write(
        work.join("r06-command.cmd"),
        include_str!("shell_fixtures/cmd.cmd").replace('\n', "\r\n"),
    )?;
    fs::write(work.join("r06-child.cmd"), "@exit /b 9\r\n")?;
    fs::write(
        work.join("r06-cmd-child.ps1"),
        format!(
            "{}\n{}",
            aura_backend_windows_user::environment::POWERSHELL_BOOTSTRAP,
            include_str!("shell_fixtures/cmd-child.ps1")
        ),
    )?;
    Ok(["/d", "/q", "/u", "/e:on", "/v:off", "/c", "r06-command.cmd"]
        .into_iter()
        .map(str::to_owned)
        .collect())
}

fn validate_cmd(execution: &Value, root: &Path, runtime: &Path) -> Result<()> {
    ensure!(
        execution["termination"] == "exited"
            && execution["exit_code"] == 0
            && execution["payload"]["shell"] == "cmd"
            && execution["payload"]["child_exit"] == 9
            && execution["payload"]["environment_revision"]
                == aura_backend_windows_user::environment::REVISION,
        "CMD execution semantics failed: {execution}"
    );
    let bytes = fs::read(root.join("allowed/cmd space/text.txt"))?;
    ensure!(bytes.len() % 2 == 0, "truncated CMD Unicode output");
    let words = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    ensure!(String::from_utf16(&words)? == "hello & world!\r\n", "CMD redirect/escaping changed");
    let child: Value =
        serde_json::from_slice(&fs::read(root.join("allowed/cmd-child-runtime.json"))?)?;
    let pin: Value = serde_json::from_str(include_str!("../../../runtimes/powershell.json"))?;
    ensure!(
        child["version"] == pin["version"]
            && child["runtime"].as_str().map(Path::new) == Some(runtime)
            && child["environment_revision"] == aura_backend_windows_user::environment::REVISION,
        "CMD child selected a different runtime"
    );
    Ok(())
}

pub(crate) fn powershell_arguments() -> Vec<String> {
    aura_backend_windows_user::environment::powershell_command(include_str!(
        "shell_fixtures/powershell.ps1"
    ))
}

pub(crate) fn validate_powershell(execution: &Value, root: &Path) -> Result<()> {
    ensure!(
        execution["termination"] == "exited" && execution["exit_code"] == 0,
        "PowerShell did not exit successfully: {execution}"
    );
    let pin: Value = serde_json::from_str(include_str!("../../../runtimes/powershell.json"))?;
    let payload = &execution["payload"];
    let child: Value =
        serde_json::from_slice(&fs::read(root.join("allowed/powershell-child-runtime.json"))?)?;
    ensure!(
        child["version"] == pin["version"] && child["modules_fixed"] == true,
        "managed child environment drifted"
    );
    ensure!(
        payload["version"] == pin["version"]
            && payload["edition"] == "Core"
            && payload["apartment"] == "MTA"
            && payload["environment_revision"] == aura_backend_windows_user::environment::REVISION,
        "observed runtime differs from managed baseline"
    );
    ensure!(
        payload["child_exit"] == 7
            && payload["child_runtime_matches"] == true
            && payload["pipeline"] == "2,4,6"
            && payload["profile_loaded"] == false
            && payload["parent_environment_leaked"] == false
            && payload["environment_paths_valid"] == true,
        "command execution semantics changed"
    );
    for (file, expected) in [
        ("powershell-child.txt", "child"),
        ("中文 空格.txt", "引号 \"hello\" 'world'"),
        ("powershell-redirect.txt", "2,4,6"),
    ] {
        ensure!(
            fs::read(root.join("allowed").join(file))? == expected.as_bytes(),
            "PowerShell postcondition differs: {file}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn required_shells_cannot_pass_from_old_language_images() {
        let language: ToolImage = serde_json::from_value(json!({
            "name":"rust","path":"never-execute-rustc.exe","sha256":"aa"
        }))
        .expect("fixed image");
        let report = run(&[language], "unused", Path::new("unused"), None);
        assert_eq!(report["schema"], SCOPE);
        assert_eq!(report["results"][0]["result"]["status"], "not_available");
        assert_eq!(report["results"][1]["result"]["status"], "not_available");
        assert_eq!(report["results"].as_array().expect("rows").len(), 2);
        assert!(
            serde_json::from_value::<ToolImage>(json!({
                "name":"powershell","path":"pwsh.exe","sha256":"aa","command":"arbitrary"
            }))
            .is_err()
        );
    }
}
