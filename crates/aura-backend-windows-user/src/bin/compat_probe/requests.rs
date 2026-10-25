use anyhow::Result;
use aura_backend_windows_user::compat::{CompatBackend, CompatJobs, LaunchFault};
use aura_windows_user_contract::{ManagedShell, ManagedShellCommand};
use serde_json::{Value, json};
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub(super) fn handle(
    backend: &CompatBackend,
    jobs: &mut CompatJobs,
    input: &Value,
) -> Result<Value> {
    let action = input["action"].as_str().unwrap_or("run");
    if action == "observe" {
        let observed = jobs.observe(
            input["id"].as_str().ok_or_else(|| anyhow::anyhow!("owned Job id required"))?,
        )?;
        return Ok(
            json!({"root_exit":observed.root_exit,"active_processes":observed.active_processes,"registered_jobs":jobs.len(),"members":observed.members.iter().map(|identity|json!({"pid":identity.process_id,"created":identity.creation_time_100ns,"image":identity.executable_path})).collect::<Vec<_>>()}),
        );
    }
    if action == "stop" {
        let began = Instant::now();
        let result = jobs.stop(
            input["id"].as_str().ok_or_else(|| anyhow::anyhow!("owned Job id required"))?,
            Duration::from_millis(input["bound_ms"].as_u64().unwrap_or(3000)),
        )?;
        return Ok(
            json!({"confirmed":result.confirmed,"active_processes":result.active_processes,"error":result.error,"elapsed_ms":began.elapsed().as_secs_f64()*1000.0,"registered_jobs":jobs.len()}),
        );
    }
    let shell = match input["shell"].as_str() {
        Some("powershell") => ManagedShell::PowerShell,
        Some("cmd") => ManagedShell::Cmd,
        _ => anyhow::bail!("unknown fixture Shell"),
    };
    let command = ManagedShellCommand {
        shell,
        script: input["script"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("fixture script required"))?
            .into(),
        working_directory: input["working_directory"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("owned directory required"))?
            .into(),
        stdin: input["stdin"].as_str().unwrap_or_default().into(),
        timeout_ms: input["timeout_ms"].as_u64().unwrap_or(5000),
        max_output_bytes: input["max_output_bytes"].as_u64().unwrap_or(65536),
    };
    let mut execution = if let Some(fault) = input["fault"].as_str() {
        backend.launch_with_fault(
            &command,
            match fault {
                "assignment" => LaunchFault::Assignment,
                "resume" => LaunchFault::Resume,
                _ => anyhow::bail!("unknown fault"),
            },
        )?
    } else {
        backend.launch(&command, input["offline_proxy"].as_str())?
    };
    let identity = execution.root_identity().clone();
    let direct =
        execution.run_direct(&AtomicBool::new(input["cancelled"].as_bool().unwrap_or(false)))?;
    let (timing, prepared) = execution.launch_timing();
    let stages = json!({"prepared_ms":prepared.as_secs_f64()*1000.0,"before_create_ms":timing.before_create.as_secs_f64()*1000.0,"created_ms":timing.created.as_secs_f64()*1000.0,"assigned_ms":timing.assigned.as_secs_f64()*1000.0,"resumed_ms":timing.resumed.as_secs_f64()*1000.0});
    let id = if direct.active_processes > 0 { Some(jobs.register(execution)?) } else { None };
    Ok(
        json!({"exit_code":direct.exit_code,"stdout":String::from_utf8_lossy(&direct.stdout),"stderr":String::from_utf8_lossy(&direct.stderr),"output_truncated":direct.output_truncated,"retained_bytes":direct.stdout.len()+direct.stderr.len(),"active_processes":direct.active_processes,"stop_reason":direct.stop_reason,"stop_confirmed":direct.stop_confirmed,"stdin_completed":direct.stdin_completed,"stdin_error":direct.stdin_error,"elapsed_ms":direct.elapsed.as_secs_f64()*1000.0,"marker_ms":direct.marker.map(|duration|duration.as_secs_f64()*1000.0),"stages":stages,"root_pid":identity.process_id,"root_created":identity.creation_time_100ns,"background_id":id,"registered_jobs":jobs.len()}),
    )
}

pub(super) fn failure(error: &anyhow::Error) -> Value {
    let launch = error.downcast_ref::<aura_windows_platform::JobLaunchError>();
    json!({"error":error.to_string(),"failure_stopped":launch.map(|error|error.timing.failure_stopped),"failed_root":launch.and_then(|error|error.timing.root_identity.as_ref()).map(|identity|json!({"pid":identity.process_id,"created":identity.creation_time_100ns}))})
}
