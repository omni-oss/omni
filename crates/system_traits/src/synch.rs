use std::collections::BTreeMap;
use std::env::{Vars, VarsOs};

pub use sys_traits::{
    BaseEnvSetCurrentDir, BaseEnvSetVar, BaseEnvVar, BaseFsCanonicalize,
    BaseFsChown, BaseFsCloneFile, BaseFsCopy, BaseFsCreateDir,
    BaseFsCreateJunction, BaseFsHardLink, BaseFsMetadata, BaseFsOpen,
    BaseFsRead, BaseFsReadDir, BaseFsReadLink, BaseFsRemoveDir,
    BaseFsRemoveDirAll, BaseFsRemoveFile, BaseFsRename, BaseFsSetFileTimes,
    BaseFsSetPermissions, BaseFsSetSymlinkFileTimes, BaseFsSymlinkChown,
    BaseFsSymlinkDir, BaseFsSymlinkFile, BaseFsWrite, BoxableFsFile,
    CreateDirOptions, EnvCacheDir, EnvCurrentDir, EnvHomeDir, EnvProgramsDir,
    EnvSetCurrentDir, EnvSetUmask, EnvSetVar, EnvTempDir, EnvUmask, EnvVar,
    FileType, FsCanonicalize, FsChown, FsCloneFile, FsCopy, FsCreateDir,
    FsCreateDirAll, FsCreateJunction, FsDirEntry, FsFile, FsFileAsRaw,
    FsFileIsTerminal, FsFileLock, FsFileLockMode, FsFileMetadata, FsFileSetLen,
    FsFileSetPermissions, FsFileSetTimes, FsFileSyncAll, FsFileSyncData,
    FsFileTimes, FsHardLink, FsMetadata, FsOpen, FsRead, FsReadDir, FsReadLink,
    FsRemoveDir, FsRemoveDirAll, FsRemoveFile, FsRename, FsSetFileTimes,
    FsSetPermissions, FsSetSymlinkFileTimes, FsSymlinkChown, FsSymlinkDir,
    FsSymlinkFile, FsWrite, SystemRandom, SystemTimeNow, ThreadSleep,
};

pub trait EnvVars {
    fn env_vars(&self) -> Vars;

    fn env_vars_os(&self) -> VarsOs;
}

/// Materializes the process environment into an owned, inspectable map.
///
/// [`EnvVars::env_vars`] is bound to return `std::env::Vars`, which has no
/// public constructor and therefore **cannot be filtered in place**. Consumers
/// that must apply a policy to the environment before exposing it to an
/// untrusted script materialize it through this trait instead. The blanket
/// implementation simply collects the full environment; a wrapper that confines
/// environment access overrides it to drop disallowed variables (see
/// `omni_capability_sys::PolicyEnforcingSys`).
pub trait EnvSnapshot {
    fn env_snapshot(&self) -> BTreeMap<String, String>;
}

impl<T: EnvVars> EnvSnapshot for T {
    fn env_snapshot(&self) -> BTreeMap<String, String> {
        self.env_vars().collect()
    }
}
