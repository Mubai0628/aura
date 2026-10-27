//! Audited Windows-only native boundary for Aura.
//!
//! Every other Aura crate forbids unsafe code. Keep all Win32 FFI calls in this
//! crate and expose safe, purpose-specific APIs to the rest of the workspace.

#[cfg(not(windows))]
compile_error!("aura-windows-platform supports Windows 11 only");

#[cfg(feature = "restricted-user-runtime")]
mod account_secret;
#[cfg(feature = "restricted-user-runtime")]
mod execution_metadata;
mod user_protection;
#[cfg(feature = "restricted-user-runtime")]
pub use execution_metadata::ExecutionMetadataAccess;
mod atomic_file;
#[cfg(feature = "restricted-user-runtime")]
pub use account_secret::decrypt_execution_account_secret;
pub use user_protection::{protect_for_current_user, unprotect_for_current_user};
#[cfg(feature = "restricted-user-runtime")]
mod account_network;
mod background_work;
pub use background_work::{enter_background_mode, lower_cpu_priority};
mod bootstrap;
mod capability;
mod child_environment;
mod credential;
mod dialog;
mod directory_create;
mod disk_space;
mod dns;
mod elevation;
#[cfg(feature = "execution-maintenance")]
pub mod execution_maintenance;
#[cfg(feature = "experimental-processmodel-proxy")]
mod experimental_process_sandbox;
mod file_identity;
mod file_name;
mod git_installation;
pub mod installer;
mod progress_dialog;
mod system_load;
pub use git_installation::machine_git_installation;
mod handle;
mod headless_web;
mod host_preparation;
mod job;
mod path;
mod pipe;
mod process;
mod release_trust;
mod sandbox;
pub use sandbox::current_user_environment;
pub use sandbox::{NativeTerminal, NativeTerminalLaunch, TerminalOutputCallback, TerminalSize};
mod process_children;
// The model's deletions go to the Recycle Bin, never for good.
mod recycle;
pub use process_children::child_process_identities;
pub use recycle::{RecycleOutcome, move_to_recycle_bin};
mod security;
mod system_cmd;
mod system_path;
mod user_profile;
pub use user_profile::user_profile_directory;
mod watcher;
#[cfg(feature = "restricted-user-runtime")]
pub use account_network::{AccountNetworkPolicy, AccountNetworkSpec, manage_account_network};
#[cfg(feature = "restricted-user-spike")]
pub use account_network::{inspect_spike_rpc_guard, spike_rpc_guard_plan};
pub use capability::{
    CapabilityDevice, capability_sid, is_capability_sid_shape, null_capability_sid,
};
#[cfg(feature = "restricted-user-runtime")]
pub use host_preparation::execution_device::ExecutionDevice;
pub mod installation_identity;

/// Named, default-off fault seams for the publication sequence.
///
/// Exported so the acceptance binary exercises the **same** production code path it is testing
/// rather than a test-only copy of it. Nothing here is armed unless a caller arms it.
pub use atomic_file::fault as publication_fault;
#[cfg(test)]
mod held_move_tests;
pub use atomic_file::{
    AtomicReplaceError, ContentHashAlgorithm, HandleBoundPublishedFile, HandleBoundReplaceContent,
    HandlePublishError, PublishLocation, StagedFilePublication, atomic_replace_file,
    handle_bound_create_absent, handle_bound_create_new, handle_bound_delete,
    handle_bound_move_existing, handle_bound_replace, handle_bound_replace_checked,
    handle_bound_replace_verified, handle_bound_replace_with_backup,
    handle_bound_replace_with_backup_checked,
};
pub use bootstrap::{BOOTSTRAP_SECRET_BYTES, read_inherited_bootstrap_secret};
pub use credential::{
    CredentialApplicationAttribute, CredentialStoreError,
    MAX_CREDENTIAL_ATTRIBUTE_KEYWORD_UTF16_UNITS, MAX_CREDENTIAL_ATTRIBUTE_VALUE_BYTES,
    MAX_CREDENTIAL_ATTRIBUTES, MAX_CREDENTIAL_BYTES, SecretBytes, StoredCredential,
    WindowsCredentialStore,
};
pub use dialog::{
    RecoveryAction, RecoveryChoice, choose_diagnostic_export_path, show_recovery_dialog,
};
pub use directory_create::handle_bound_create_directory;
pub use disk_space::free_disk_space;
pub use dns::{DnsAddressRecord, resolve_dns_addresses};
pub use elevation::{
    ElevatedHelperLaunch, ElevatedHelperLaunchError, ElevatedProcess,
    launch_verified_aura_elevated_helper,
};
#[cfg(feature = "experimental-processmodel-proxy")]
pub use experimental_process_sandbox::{
    EXPERIMENTAL_PROCESSMODEL_DOCUMENTATION_URL, ExperimentalProcessModelProxyProbe,
    ExperimentalProcessModelProxyStatus, probe_experimental_processmodel_proxy,
};
pub use file_identity::{
    ChangeEpoch, ChangeWitness, ChangeWitnessOutcome, IdentityLookup, change_generation,
    epoch_advanced, probe_change_epoch, resolve_file_identity, stamp_change_epoch,
    witness_change_generation,
};
pub use file_name::directory_name_comparison;
pub use headless_web::{
    HeadlessFinish, HeadlessPage, HeadlessRenderError, HeadlessRenderSession, HeadlessScript,
    RenderAbort, RenderOutcome, RenderedHtml, render_fetched_html, render_fetched_page,
    start_headless_render, start_headless_render_cancellable,
};
pub use host_preparation::{
    ALL_APPLICATION_PACKAGES_SID, ALL_RESTRICTED_APPLICATION_PACKAGES_SID, HostCompatibilityProbe,
    NULL_DEVICE_COMPATIBILITY_SDDL, SYSTEM_DRIVE_COMPATIBILITY_MASK,
    SystemDriveCompatibilityChanges, VerifiedCapabilityAce,
    capture_null_device_security_descriptor,
    dacl_ace::{
        DaclAceIdentity, DaclObservationV1, DaclTransitionV1, DaclWriteOperationV1,
        classify_control_transition, dacl_control_letters, ensure_only_added_ace,
        ordered_ace_identities, ordered_ace_sequences_match, parse_dacl_ace_identities,
    },
    null_device_security_descriptor_matches, prepare_null_device_compatibility,
    prepare_system_drive_compatibility, prepare_system_drive_compatibility_with_transition,
    probe_host_compatibility, probe_null_device_compatibility,
    probe_null_device_compatibility_limited, probe_system_drive_compatibility,
    remove_system_drive_compatibility, remove_system_drive_compatibility_with_transition,
    restore_null_device_security_descriptor, system_drive_root,
};
#[cfg(feature = "restricted-user-runtime")]
pub use job::AccountRunnerLaunch;
pub use job::{ComponentJob, JobCompletionNotification, JobLimits, ManagedProcess, RootJob};
pub use path::{
    NativeFileAccess, NativeFileIdentity, NativePathInfo, SearchSource, VerifiedFile,
    VerifiedToolOutputFile, VolumeCaseRule, directory_case_rule, directory_case_sensitivity_flag,
    extended_length_path, final_path_within, held_file_identity, inspect_existing_path,
    lock_execution_resources, open_execution_metadata, open_held_for_move, open_held_for_read,
    open_verified_executable, open_verified_existing, open_verified_import_source,
    open_verified_publish_parent, open_verified_search_source, open_verified_tool_output_from_root,
    open_verified_tool_output_root, open_verified_tool_output_source, volume_case_rule,
    volume_root_of, windows_ordinal_compare_ignore_case,
};
#[cfg(feature = "restricted-user-runtime")]
pub use pipe::create_execution_byte_pipe_server;
pub use pipe::{create_secure_byte_pipe_server, open_secure_byte_pipe_client};
#[cfg(feature = "restricted-user-runtime")]
pub use process::process_account_sid;
pub use process::{
    ProcessIdentity, ProcessWorkingSet, RetainedProcess, current_process_thread_count,
    current_process_working_set, named_pipe_client_identity, named_pipe_server_identity,
    retain_process, snapshot_process, validate_named_pipe_client, validate_named_pipe_server,
};
pub use progress_dialog::{Progress, with_progress_window};
pub use release_trust::{
    AuthenticatedActiveCoreIdentity, authenticate_active_core_identity,
    verify_active_release_trust, verify_release_manifest, verify_release_manifest_against_root,
};
#[cfg(feature = "restricted-user-runtime")]
pub use sandbox::ExecutionFileSecurity;
pub use sandbox::verify_execution_installation_acl;
#[cfg(feature = "restricted-user-runtime")]
pub use sandbox::verify_execution_maintenance_acl;
pub use sandbox::{
    AppContainerProfile, NativeAclAccess, NativeEnvironmentEntry, NativeProcessStdin,
    NativeRestrictedMode, NativeSandboxProcess, NativeSecretEnvironmentEntry, WindowsSandboxProbe,
    capture_file_dacl, current_process_is_appcontainer, current_process_is_elevated,
    delete_appcontainer_profile, dos_drive_path, grant_file_dacl, probe_windows_sandbox,
    restore_file_dacl, revoke_file_dacl, spawn_restricted_process,
    spawn_restricted_process_with_secret_environment, spawn_standard_user_process,
};
pub use sandbox::{JobLaunchError, JobLaunchTiming, spawn_compat_process};
pub use system_load::{CpuLoad, PhysicalMemory, physical_memory};
mod git_read;
#[cfg(feature = "git-read-test-profile")]
pub use git_read::set_git_read_fixture_integrity;
pub use git_read::{GitReadDirectory, spawn_git_read_process};
#[cfg(any(test, feature = "compat-adoption"))]
pub use sandbox::{JobLaunchFault, spawn_compat_process_with_fault};
pub use system_cmd::{verify_microsoft_catalog_executable, windows_system_component_policy};
pub use system_path::{
    common_application_data_directory, windows_directory, windows_powershell_executable,
};
pub use watcher::{FileWatchBackend, FileWatchEvent, FileWatchEventKind, WorkspaceFileWatcher};

#[cfg(feature = "restricted-user-runtime")]
pub use sandbox::restricted_user::{
    RestrictedUserLaunchOptions, RestrictedUserProcess, current_user_token_identity,
    current_windows_session_id, execution_namespace_access, execution_namespace_dacl,
    execution_namespace_sid, grant_empty_execution_write_root, spawn_restricted_user_process,
    suppress_execution_error_dialogs,
};
#[cfg(feature = "restricted-user-spike")]
pub use sandbox::restricted_user::{spike_runtime_dlls, spike_synchronization};
