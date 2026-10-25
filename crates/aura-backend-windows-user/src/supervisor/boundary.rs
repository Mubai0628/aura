//! The OS, process and clock edges one launch touches, drawn as a single
//! boundary. The production implementation delegates each method to the one
//! platform call it names; chain regressions substitute scripted gates at
//! exactly these edges and still drive the real supervisor sequence. No
//! ordering, policy or retry lives here — that stays in `supervisor::run`.
use crate::{file_lease::FileLease, runtime::ResourceCounts, work::LaunchWorkV1};
use anyhow::{Result, ensure};
use aura_domain_common::{ContentHash, UtcTimestamp};
use aura_windows_platform::{
    ExecutionMetadataAccess, ManagedProcess, NativeEnvironmentEntry, ProcessIdentity, RootJob,
    SecretBytes,
};
use aura_windows_user_contract::{
    CoreRunnerMessage, ExecutionAccount, ExecutionFileBinding, RunnerCoreMessage,
};
use std::{
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

/// The installation facts one launch binds to. Read from the verified
/// `Installation` by the controller; a chain regression builds the same facts
/// from a synthetic layout, so the supervisor never opens an installation.
#[derive(Clone)]
pub(crate) struct LaunchFacts {
    pub root: PathBuf,
    pub installation_id: String,
    pub owner_sid: String,
    pub account: ExecutionAccount,
    pub runner_image: ExecutionFileBinding,
}

pub(crate) type TransportFuture<'a, T> =
    Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>;

/// The byte transport to the already-spawned Runner. Implementations pair the
/// execution pipe with the process it talks to; `split_for_execute` is the
/// Execute boundary, after which reads and writes run concurrently.
pub(crate) trait RunnerTransport: Send {
    /// The launched process's exact identity facts.
    fn identity(&self) -> Result<ProcessIdentity>;
    /// Wait for the Runner to connect to the pipe.
    fn connect<'a>(&'a mut self) -> TransportFuture<'a, ()>;
    /// Fail-closed client authentication after connect.
    fn authenticate_client(&self) -> Result<()>;
    /// The one pre-Execute frame: the Runner's Ready or Rejected.
    fn read_ready<'a>(&'a mut self) -> TransportFuture<'a, RunnerCoreMessage>;
    /// Cross the Execute boundary: what follows is concurrent read/write.
    fn split_for_execute(self: Box<Self>) -> (Box<dyn RunnerReader>, Box<dyn RunnerWriter>);
}

pub(crate) trait RunnerReader: Send {
    fn read<'a>(&'a mut self) -> TransportFuture<'a, RunnerCoreMessage>;
}

pub(crate) trait RunnerWriter: Send {
    fn write<'a>(&'a mut self, message: &'a CoreRunnerMessage) -> TransportFuture<'a, ()>;
}

/// Everything the spawn needs that is not already a supervisor local.
pub(crate) struct RunnerLaunch<'a> {
    pub job: &'a RootJob,
    pub pipe_name: &'a str,
    pub account_name: &'a str,
    pub account_sid: &'a str,
    pub password: &'a SecretBytes,
    pub executable: &'a Path,
    pub arguments: &'a [String],
    pub directory: &'a Path,
    pub environment: &'a [NativeEnvironmentEntry],
}

pub(crate) trait LaunchBoundary: Send + Sync {
    /// The wall clock authority decisions read. Durations stay on the monotonic
    /// clock; this is the only wall clock a launch consults.
    fn now(&self) -> UtcTimestamp;

    /// Verify the installation's protected boundaries and unwrap the account
    /// credential, counting the three ACL checks and hashed bytes into `work`.
    fn verify_installation(
        &self,
        facts: &LaunchFacts,
        online: bool,
        work: &mut LaunchWorkV1,
    ) -> Result<SecretBytes>;

    /// Whether the runtime's lock-and-recovery block was already open.
    fn runtime_reused(&self) -> bool;

    /// The recovery block's measured cost, exactly once per runtime open, by
    /// the launch whose allocation paid it. `None` on a reused runtime.
    fn take_recovery_work(&self) -> Option<crate::work::RecoveryWork>;

    /// Live resource counts at the moment of allocation.
    fn resource_counts(&self) -> ResourceCounts;

    /// Register one resource unit and run `prepare` against its lease. The unit
    /// stays registered even when `prepare` fails: a partial preparation remains
    /// owned until [`Self::stop_resources`] restores it.
    fn allocate_resources(
        &self,
        facts: &LaunchFacts,
        nonce: &str,
        account_sid: &str,
        read_roots: &[ExecutionFileBinding],
        prepare: &mut dyn FnMut(&mut FileLease, &mut Option<ExecutionFileBinding>) -> Result<()>,
    ) -> Result<Arc<RootJob>>;

    /// Grant the authenticated Runner's ancestor metadata access on the lease.
    fn prepare_workspace(
        &self,
        nonce: &str,
        prepare: &mut dyn FnMut(&mut FileLease) -> Result<()>,
    ) -> Result<()>;

    /// Stop one unit's background tree and restore its lease; `true` only when
    /// the runtime drained and every remaining lease restored.
    fn stop_resources(&self, nonce: &str) -> Result<bool>;

    /// Authenticated metadata view of the Runner token, used by the ancestor
    /// grant. Only called after the transport authenticated the client.
    fn runner_metadata_access(
        &self,
        identity: &ProcessIdentity,
        account_sid: &str,
    ) -> Result<ExecutionMetadataAccess>;

    /// Create the execution pipe and spawn the account Runner into `job`.
    fn launch_runner(&self, launch: &RunnerLaunch<'_>) -> Result<Box<dyn RunnerTransport>>;
}

pub(crate) struct SystemBoundary {
    binding: Arc<crate::installation::InstallationBinding>,
}

impl SystemBoundary {
    pub(crate) fn new(binding: Arc<crate::installation::InstallationBinding>) -> Self {
        Self { binding }
    }
}

impl LaunchBoundary for SystemBoundary {
    fn now(&self) -> UtcTimestamp {
        UtcTimestamp::now()
    }

    fn verify_installation(
        &self,
        facts: &LaunchFacts,
        online: bool,
        work: &mut LaunchWorkV1,
    ) -> Result<SecretBytes> {
        let journal_root = facts.root.join("private/execution-leases");
        aura_windows_platform::verify_execution_installation_acl(
            &facts.root.join("private"),
            &facts.owner_sid,
            true,
        )?;
        work.record_installation_check();
        aura_windows_platform::verify_execution_installation_acl(
            &journal_root,
            &facts.owner_sid,
            true,
        )?;
        work.record_installation_check();
        let (_, _journal_root) = crate::file_binding::capture(&journal_root, false)?;
        let private =
            if online { "private/online-credential.dpapi" } else { "private/credential.dpapi" };
        aura_windows_platform::verify_execution_installation_acl(
            &facts.root.join(private),
            &facts.owner_sid,
            true,
        )?;
        work.record_installation_check();
        let (binding, mut protected) =
            crate::file_binding::capture(&facts.root.join(private), true)?;
        ensure!(
            binding.sha256.as_ref() == Some(&facts.account.credential_sha256),
            "runner.account_credential_changed"
        );
        use std::io::{Read as _, Seek as _};
        protected.file_mut().rewind()?;
        let mut cipher = Vec::new();
        protected.file_mut().take(16_385).read_to_end(&mut cipher)?;
        ensure!(
            cipher.len() <= 16_384
                && ContentHash::sha256(&cipher) == facts.account.credential_sha256,
            "runner.account_credential_changed"
        );
        work.record_hashed_bytes(cipher.len() as u64);
        Ok(aura_windows_platform::decrypt_execution_account_secret(&cipher)?)
    }

    fn runtime_reused(&self) -> bool {
        self.binding.resources.is_open()
    }

    fn take_recovery_work(&self) -> Option<crate::work::RecoveryWork> {
        self.binding.resources.take_recovery_work()
    }

    fn resource_counts(&self) -> ResourceCounts {
        self.binding.resources.resource_counts()
    }

    fn allocate_resources(
        &self,
        facts: &LaunchFacts,
        nonce: &str,
        account_sid: &str,
        read_roots: &[ExecutionFileBinding],
        prepare: &mut dyn FnMut(&mut FileLease, &mut Option<ExecutionFileBinding>) -> Result<()>,
    ) -> Result<Arc<RootJob>> {
        self.binding.resources.allocate(facts, nonce, account_sid, read_roots, prepare)
    }

    fn prepare_workspace(
        &self,
        nonce: &str,
        prepare: &mut dyn FnMut(&mut FileLease) -> Result<()>,
    ) -> Result<()> {
        self.binding.resources.prepare_workspace(nonce, prepare)
    }

    fn stop_resources(&self, nonce: &str) -> Result<bool> {
        self.binding.resources.stopped(nonce)
    }

    fn runner_metadata_access(
        &self,
        identity: &ProcessIdentity,
        account_sid: &str,
    ) -> Result<ExecutionMetadataAccess> {
        Ok(ExecutionMetadataAccess::for_runner(identity, account_sid)?)
    }

    fn launch_runner(&self, launch: &RunnerLaunch<'_>) -> Result<Box<dyn RunnerTransport>> {
        let pipe = aura_windows_platform::create_execution_byte_pipe_server(
            launch.pipe_name,
            launch.account_sid,
        )?;
        let process =
            launch.job.spawn_account_runner(aura_windows_platform::AccountRunnerLaunch {
                account_name: launch.account_name,
                password: launch.password,
                executable: launch.executable,
                arguments: launch.arguments,
                directory: launch.directory,
                environment: launch.environment,
            })?;
        Ok(Box::new(SystemRunnerTransport { pipe, process }))
    }
}

struct SystemRunnerTransport {
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    process: ManagedProcess,
}

impl RunnerTransport for SystemRunnerTransport {
    fn identity(&self) -> Result<ProcessIdentity> {
        Ok(self.process.identity()?)
    }

    fn connect<'a>(&'a mut self) -> TransportFuture<'a, ()> {
        Box::pin(async move {
            self.pipe.connect().await?;
            Ok(())
        })
    }

    fn authenticate_client(&self) -> Result<()> {
        let identity = self.identity()?;
        aura_windows_platform::validate_named_pipe_client(&self.pipe, &identity)?;
        Ok(())
    }

    fn read_ready<'a>(&'a mut self) -> TransportFuture<'a, RunnerCoreMessage> {
        Box::pin(async move { crate::wire::read(&mut self.pipe).await })
    }

    fn split_for_execute(self: Box<Self>) -> (Box<dyn RunnerReader>, Box<dyn RunnerWriter>) {
        let (reader, writer) = tokio::io::split(self.pipe);
        (
            Box::new(SystemRunnerReader { inner: reader }),
            Box::new(SystemRunnerWriter { inner: writer }),
        )
    }
}

struct SystemRunnerReader {
    inner: tokio::io::ReadHalf<tokio::net::windows::named_pipe::NamedPipeServer>,
}

impl RunnerReader for SystemRunnerReader {
    fn read<'a>(&'a mut self) -> TransportFuture<'a, RunnerCoreMessage> {
        Box::pin(async move { crate::wire::read(&mut self.inner).await })
    }
}

struct SystemRunnerWriter {
    inner: tokio::io::WriteHalf<tokio::net::windows::named_pipe::NamedPipeServer>,
}

impl RunnerWriter for SystemRunnerWriter {
    fn write<'a>(&'a mut self, message: &'a CoreRunnerMessage) -> TransportFuture<'a, ()> {
        Box::pin(async move { crate::wire::write(&mut self.inner, message).await })
    }
}
