use super::{
    outputs::{self, VerifiedProgramArtifact},
    protocol::Protocol,
};
use anyhow::{Result, ensure};
use aura_backend_windows_user::program::{
    ProgramEnvironment, ProgramInput, ProgramSpec, ProgramStop,
};
use aura_domain_common::{
    ToolCallId, ToolExecutionAttemptId,
    jcs::{CanonicalSha256HashV1, canonical_sha256},
};
use aura_tool_contract::*;
use aura_tool_domain::{
    CanonicalToolPackageRuntimeV1, PackageCapabilityRequirementV1,
    PackageConformanceReasonV1 as Reason,
};
use aura_tool_package::StagedToolPackageV1;
use aura_tool_runtime::{
    CommandRuntimeContextV1, PreparedCommandRuntimeKindV1, PreparedToolResultContractV1,
    prepare_tool_sandbox_runtime_v1,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

/// All paths/environment here are supplied by Core after user/resource checks,
/// never deserialized from model arguments. Manifest mappings are still validated.
#[derive(Clone)]
pub struct ProgramContext<'a> {
    pub identity: ProgramInvocationIdentity,
    pub workspace_root: Option<&'a Path>,
    pub directory_claims: BTreeMap<PackageResourceClaimKeyV1, PathBuf>,
    pub instance_configuration: Option<&'a serde_json::Value>,
    /// The same fully verified managed runtime used by Shell. The borrowed
    /// backend retains every pinned DLL/script/image until this call ends.
    pub powershell: Option<&'a aura_backend_windows_user::compat::CompatBackend>,
    pub user_environment: Vec<(OsString, OsString)>,
    /// A Core-owned run parent. Its owner must recover leftover child roots
    /// after a Core crash; this primitive does not enumerate unrelated TEMP data.
    pub scratch_parent: &'a Path,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

/// Allocated by the owning Core attempt, never by a runtime or model argument.
/// Conformance allocates distinct identities for its own synthetic attempts.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ProgramInvocationIdentity {
    pub tool_call_id: ToolCallId,
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
}

pub struct ProgramRunResult {
    pub identity: ProgramInvocationIdentity,
    pub reason: Reason,
    pub started: bool,
    pub stop_confirmed: bool,
    /// Candidate body has passed the declared result contract only on Completed.
    pub body: Vec<u8>,
    pub artifacts: Vec<VerifiedProgramArtifact>,
    pub runtime_evidence_hash: CanonicalSha256HashV1,
}

pub fn run_case(
    staged: &StagedToolPackageV1,
    tool_key: &ToolKey,
    arguments: &serde_json::Value,
    context: &ProgramContext<'_>,
    cancelled: &AtomicBool,
    on_started: &mut dyn FnMut(u32),
) -> Result<ProgramRunResult> {
    let scratch =
        tempfile::Builder::new().prefix("package-call-").tempdir_in(context.scratch_parent)?;
    let mut started = false;
    let result =
        execute(staged, tool_key, arguments, context, cancelled, scratch.path(), &mut |pid| {
            started = true;
            on_started(pid)
        });
    // This runs on both preparation errors and post-launch outcomes. Dropping
    // TempDir silently is not enough evidence to claim cleanup succeeded.
    let cleaned = scratch.close().is_ok();
    let result = match result {
        Ok(mut result) => {
            if !cleaned {
                result.reason = Reason::CleanupUnconfirmed;
            }
            result.runtime_evidence_hash = canonical_sha256(
                &serde_json::json!({"execution":result.runtime_evidence_hash,"reason":result.reason,"scratch_cleaned":cleaned}),
            )?;
            Ok(result)
        }
        Err(error) if !cleaned => Err(error.context("package.cleanup_unconfirmed")),
        Err(error) => Err(error),
    };
    result.map_err(|error| {
        error.context(if started {
            "package.execution_report_unconfirmed"
        } else {
            "package.preparation_failed"
        })
    })
}

fn execute(
    staged: &StagedToolPackageV1,
    tool_key: &ToolKey,
    arguments: &serde_json::Value,
    context: &ProgramContext<'_>,
    cancelled: &AtomicBool,
    scratch: &Path,
    on_started: &mut dyn FnMut(u32),
) -> Result<ProgramRunResult> {
    staged.verify_integrity()?;
    let tool = staged
        .canonical_manifest()
        .tools
        .iter()
        .find(|tool| tool.tool_key == *tool_key)
        .ok_or_else(|| anyhow::anyhow!("package.tool_missing"))?;
    ensure!(
        !matches!(tool.runtime, CanonicalToolPackageRuntimeV1::DeclarativeHttp { .. }),
        "package.program_runtime_required"
    );
    ensure!(
        tool.capability_requirements.contains(&PackageCapabilityRequirementV1::ProcessExecute),
        "package.process_capability_required"
    );
    let validator = CompiledToolSchemaV1::compile(tool.model.input_schema.canonical_document())?;
    ensure!(validator.validate(arguments).valid, "package.arguments_invalid");
    match (&tool.instance_configuration.0, context.instance_configuration) {
        (Some(schema), Some(value)) => ensure!(
            CompiledToolSchemaV1::compile(schema.canonical_document())?.validate(value).valid,
            "package.configuration_invalid"
        ),
        (None, None) => {}
        _ => anyhow::bail!("package.configuration_required"),
    }
    let runtime = tool.runtime.to_runtime_descriptor_v1();
    let directory = match &runtime {
        ToolRuntimeDescriptorV1::AuraStdio { working_directory, .. } => working_directory,
        ToolRuntimeDescriptorV1::DeclarativeCommand { command } => match command {
            DeclarativeCommandV1::Argv { working_directory, .. }
            | DeclarativeCommandV1::PowershellScript { working_directory, .. } => working_directory,
        },
        _ => anyhow::bail!("package.program_runtime_required"),
    };
    ensure!(
        !matches!(directory, CommandWorkingDirectoryV1::WorkspaceRoot)
            || context.workspace_root.is_some(),
        "package.workspace_required"
    );
    let system = aura_windows_platform::windows_directory()?;
    let system = system.to_str().ok_or_else(|| anyhow::anyhow!("package.system_path_invalid"))?;
    let null = serde_json::Value::Null;
    let mut prepared = prepare_tool_sandbox_runtime_v1(
        &runtime,
        arguments,
        &CommandRuntimeContextV1 {
            package_root: staged.root(),
            workspace_root: context.workspace_root.unwrap_or(staged.root()),
            resolved_directory_claims: context.directory_claims.clone(),
            instance_configuration: context.instance_configuration.unwrap_or(&null),
            system_root: system,
            windows_directory: system,
            trusted_powershell_executable: context
                .powershell
                .map(|runtime| runtime.powershell_path()),
        },
    )?;
    // D1: the Native exception never covers credential isolation, so a program tool
    // never receives an Aura-held credential.
    ensure!(prepared.credential_slot.is_none(), "package.program_credentials_unsupported");
    if prepared.runtime_kind == PreparedCommandRuntimeKindV1::PowershellScript {
        prepared
            .arguments
            .splice(0..0, ["-MTA", "-NoLogo", "-ExecutionPolicy", "Bypass"].map(str::to_owned));
    }
    let result_contract = PreparedToolResultContractV1::compile(&tool.model.result_contract)?;
    let output = scratch.join("output");
    std::fs::create_dir(&output)?;
    let call = context.identity.tool_call_id;
    let attempt = context.identity.tool_execution_attempt_id;
    let interactive = prepared.runtime_kind == PreparedCommandRuntimeKindV1::AuraStdio;
    let mut declared = prepared
        .environment
        .iter()
        .map(|entry| (entry.name.clone(), entry.value.clone()))
        .collect::<Vec<_>>();
    if interactive {
        declared.extend([
            (AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1.into(), call.to_string()),
            (
                AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1.into(),
                attempt.to_string(),
            ),
            (
                AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1.into(),
                output
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("package.output_path_invalid"))?
                    .to_owned(),
            ),
        ]);
    }
    let network = program_declares_network(tool);
    let environment = ProgramEnvironment::from_block(
        context.user_environment.clone(),
        &declared,
        network,
        scratch,
    )?;
    // The package's program is the user's own: read in full the first time, then only compared
    // until it changed or failed.
    let (image, _image) = aura_backend_windows_user::checked_program::capture(
        &prepared.executable,
        aura_backend_windows_user::checked_program::Links::Single,
    )?;
    // Deny-write handles for every authored file survive until the Job ends.
    let _manifest = aura_windows_platform::open_verified_import_source(
        &staged.root().join(TOOL_PACKAGE_MANIFEST_NAME),
        aura_windows_platform::NativeFileAccess::Read,
    )?;
    let _files = staged
        .canonical_manifest()
        .files
        .iter()
        .map(|file| {
            aura_windows_platform::open_verified_import_source(
                &staged.root().join(file.path.as_str()),
                aura_windows_platform::NativeFileAccess::Read,
            )
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    staged.verify_integrity()?;
    let mut protocol =
        interactive.then(|| Protocol::new(call, attempt, arguments.clone())).transpose()?;
    let observed = aura_backend_windows_user::program::run(
        ProgramSpec {
            image: &image,
            arguments: &prepared.arguments,
            working_directory: &prepared.working_directory,
            environment: environment.entries(),
            input: if interactive { ProgramInput::Frames } else { ProgramInput::Closed },
            cancelled,
            timeout: context.timeout,
            max_output_bytes: context.max_output_bytes,
            limits: None,
        },
        &mut |bytes: &[u8]| protocol.as_mut().map_or(Ok(vec![]), |protocol| protocol.accept(bytes)),
        on_started,
    )?;
    let mut reason = if !observed.stop_confirmed {
        Reason::CleanupUnconfirmed
    } else {
        match observed.reason {
            ProgramStop::Cancelled => Reason::Cancelled,
            ProgramStop::TimedOut => Reason::TimedOut,
            ProgramStop::OutputLimit => Reason::OutputLimit,
            ProgramStop::ProtocolError => Reason::InvalidProtocol,
            ProgramStop::IoError => Reason::ProcessFailed,
            ProgramStop::Exited if interactive && observed.exit_code == Some(0) => {
                Reason::Completed
            }
            ProgramStop::Exited
                if observed
                    .exit_code
                    .is_some_and(|code| prepared.success_exit_codes.contains(&(code as i32))) =>
            {
                Reason::Completed
            }
            ProgramStop::Exited => Reason::ProcessFailed,
        }
    };
    let body = if let Some(protocol) = &protocol {
        match protocol.result() {
            Ok(body) => body,
            Err(_) => {
                if reason == Reason::Completed {
                    reason = Reason::InvalidProtocol;
                }
                vec![]
            }
        }
    } else {
        observed.stdout.clone()
    };
    if reason == Reason::Completed && result_contract.validate_candidate(&body).is_err() {
        reason = Reason::InvalidResult;
    }
    let artifact_permission = tool.resource_claims.iter().any(|claim| {
        matches!(
            claim,
            PackageDeclaredResourceClaimV1::Artifact {
                selector: PackageArtifactSelectorV1::AttemptStaging { .. },
                ..
            }
        )
    });
    let artifacts =
        if let Some(protocol) = protocol.as_ref().filter(|_| reason == Reason::Completed) {
            match outputs::collect(&output, artifact_permission, &protocol.artifacts) {
                Ok(value) => value,
                Err(_) => {
                    if reason == Reason::Completed {
                        reason = Reason::InvalidArtifact;
                    }
                    vec![]
                }
            }
        } else {
            vec![]
        };
    drop((_manifest, _files, _image, environment));
    let evidence = canonical_sha256(
        &serde_json::json!({"backend":"package-user-job-v1","identity":context.identity,"image":image,"working_directory":prepared.working_directory,
        "arguments_hash":canonical_sha256(&prepared.arguments)?,"configuration_hash":canonical_sha256(&context.instance_configuration)?,
        "reason":reason,"started":observed.started,"stop_confirmed":observed.stop_confirmed,
        "stdout":CanonicalSha256HashV1::sha256(&observed.stdout),"stderr":CanonicalSha256HashV1::sha256(&observed.stderr),
        "artifacts":artifacts.iter().map(|value|&value.declaration).collect::<Vec<_>>() }),
    )?;
    Ok(ProgramRunResult {
        identity: context.identity.clone(),
        reason,
        started: observed.started,
        stop_confirmed: observed.stop_confirmed,
        body,
        artifacts,
        runtime_evidence_hash: evidence,
    })
}

/// A program that declares network gets the Shell's ordinary network; one that does not runs with the Shell's soft-offline environment.
pub fn program_declares_network(tool: &aura_tool_domain::CanonicalToolPackageToolV1) -> bool {
    tool.capability_requirements.iter().any(|cap| {
        matches!(
            cap,
            PackageCapabilityRequirementV1::PublicNetworkRead
                | PackageCapabilityRequirementV1::PrivateNetworkAccess
                | PackageCapabilityRequirementV1::DataEgress
        )
    })
}
