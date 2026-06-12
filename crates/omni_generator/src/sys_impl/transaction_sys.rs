//! A committable, transactional [`FsSys`]/[`ProcSys`] overlay.
//!
//! [`TransactionSys`] behaves like the older [`DryRunSys`](super::DryRunSys)
//! overlay: mutating operations are buffered in an in-memory layer and never
//! touch the real file system until they are explicitly committed. Reads are
//! served from the in-memory layer first and fall back to the real file system
//! (mirroring the fetched data into the in-memory layer) when a path has not
//! been touched yet.
//!
//! On top of that overlay behaviour it adds full transaction support:
//!
//! * **Transactions** – [`TransactionSys::begin_transaction`] opens a
//!   transaction. Nothing is written to the real file system until the
//!   *top-level* transaction is committed via
//!   [`TransactionSys::commit_transaction`]. Committing a nested transaction
//!   simply merges its actions into the enclosing one.
//! * **Rollback** – [`TransactionSys::rollback_transaction`] discards every
//!   action recorded since the matching [`begin_transaction`] call and rebuilds
//!   the in-memory view accordingly.
//! * **Checkpoints** – [`TransactionSys::checkpoint`] records a point in the
//!   action log. [`TransactionSys::rollback_to_checkpoint`] undoes every action
//!   recorded after that checkpoint.
//!
//! The implementation works by keeping an ordered log of every *mutating*
//! action. Committing replays the log against the real file system, while
//! rolling back truncates the log and rebuilds the in-memory overlay by
//! replaying the remaining actions. Because nothing is committed to the real
//! file system mid-transaction, the real file system is a stable base
//! throughout, so replaying the logical actions deterministically reproduces
//! the overlay state.
//!
//! [`FsSys`]: bridge_rpc_services-style aggregate bound
//! [`ProcSys`]: bridge_rpc_services-style aggregate bound

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashSet},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use path_clean::PathClean;
use system_traits::{
    BaseEnvSetCurrentDirAsync, BaseFsAppendAsync, BaseFsCopyAsync,
    BaseFsCreateDir, BaseFsCreateDirAsync, BaseFsMetadataAsync,
    BaseFsReadAsync, BaseFsReadDirAsync, BaseFsRemoveDir, BaseFsRemoveDirAll,
    BaseFsRemoveDirAllAsync, BaseFsRemoveDirAsync, BaseFsRemoveFileAsync,
    BaseFsRenameAsync, BaseFsWriteAsync, CreateDirOptions, EnvCurrentDirAsync,
    EnvVars, FileType, FsCreateDirAll, FsMetadata, FsMetadataValue, FsRead,
    FsRemoveFile, FsWrite,
    boxed::BoxedFsMetadataValue,
    impls::{InMemorySys, RealSys},
};
use tokio::sync::Mutex;

use crate::GeneratorSys;

/// Identifies a checkpoint created by [`TransactionSys::checkpoint`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Checkpoint(u64);

/// A single buffered mutating action.
#[derive(Clone, Debug)]
enum Action {
    Write {
        path: PathBuf,
        data: Vec<u8>,
    },
    Append {
        path: PathBuf,
        data: Vec<u8>,
    },
    CreateDir {
        path: PathBuf,
        options: CreateDirOptions,
    },
    RemoveFile {
        path: PathBuf,
    },
    RemoveDir {
        path: PathBuf,
    },
    RemoveDirAll {
        path: PathBuf,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
    },
    SetCurrentDir {
        path: PathBuf,
    },
}

/// The mutable, transactional state shared between every clone of a
/// [`TransactionSys`].
#[derive(Default)]
struct State {
    /// The in-memory overlay holding the current (uncommitted) view.
    overlay: InMemorySys,
    /// Paths that have been removed within the active transaction(s). These
    /// "whiteouts" shadow the real file system so that removed paths are not
    /// resurrected by fallthrough reads.
    deleted: HashSet<PathBuf>,
    /// Ordered log of every mutating action performed since the last commit.
    actions: Vec<Action>,
    /// For every currently open transaction, the length of [`State::actions`]
    /// at the time it was opened.
    tx_stack: Vec<usize>,
    /// Recorded checkpoints, paired with the action-log length at creation.
    checkpoints: Vec<(Checkpoint, usize)>,
    /// Monotonic counter used to mint unique [`Checkpoint`] ids.
    next_checkpoint: u64,
    /// Overridden current working directory, if `set_current_dir` was called.
    cwd: Option<PathBuf>,
}

/// A committable, transactional overlay over an underlying "real" system
/// handle `S` (anything implementing [`GeneratorSys`], e.g.
/// [`RealSys`](system_traits::impls::RealSys)).
///
/// Cloning a [`TransactionSys`] yields another handle to the *same* underlying
/// transactional state, so passing clones around the generator shares a single
/// buffered view.
#[derive(Clone)]
pub struct TransactionSys<S = RealSys> {
    real: Arc<S>,
    state: Arc<Mutex<State>>,
}

impl<S: GeneratorSys> TransactionSys<S> {
    /// Creates a new, empty transactional overlay over `real`.
    pub fn new(real: S) -> Self {
        Self {
            real: Arc::new(real),
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    // -- transaction control ------------------------------------------------

    /// Opens a new (possibly nested) transaction.
    pub async fn begin_transaction(&self) {
        let mut st = self.state.lock().await;
        let len = st.actions.len();
        st.tx_stack.push(len);
        log::debug!("Transaction: begin (depth {})", st.tx_stack.len());
    }

    /// Returns `true` if at least one transaction is currently open.
    pub async fn in_transaction(&self) -> bool {
        !self.state.lock().await.tx_stack.is_empty()
    }

    /// Returns the number of buffered (uncommitted) actions.
    pub async fn pending_actions(&self) -> usize {
        self.state.lock().await.actions.len()
    }

    /// Commits the innermost open transaction.
    ///
    /// Committing a nested transaction merely merges its actions into the
    /// enclosing transaction. Only when the *top-level* transaction is
    /// committed is the buffered action log replayed against the real file
    /// system.
    pub async fn commit_transaction(&self) -> io::Result<()> {
        let actions = {
            let mut st = self.state.lock().await;
            if st.tx_stack.pop().is_none() {
                return Err(invalid(
                    "commit_transaction: no active transaction",
                ));
            }

            // A nested commit keeps the actions buffered for the parent.
            if !st.tx_stack.is_empty() {
                log::debug!(
                    "Transaction: commit nested (depth {})",
                    st.tx_stack.len()
                );
                return Ok(());
            }

            log::debug!(
                "Transaction: commit top-level ({} action(s))",
                st.actions.len()
            );
            let actions = std::mem::take(&mut st.actions);
            st.checkpoints.clear();
            st.overlay = InMemorySys::default();
            st.deleted.clear();
            st.cwd = None;
            actions
        };

        flush_to_real(&*self.real, &actions).await
    }

    /// Rolls back the innermost open transaction, discarding every action
    /// recorded since it was opened.
    pub async fn rollback_transaction(&self) -> io::Result<()> {
        let mut st = self.state.lock().await;
        let start = st.tx_stack.pop().ok_or_else(|| {
            invalid("rollback_transaction: no active transaction")
        })?;

        log::debug!(
            "Transaction: rollback to action {} (was {})",
            start,
            st.actions.len()
        );
        st.actions.truncate(start);
        st.checkpoints.retain(|(_, len)| *len <= start);
        rebuild(&mut st, &*self.real).await;
        Ok(())
    }

    /// Commits *all* buffered actions to the real file system, regardless of
    /// the current transaction nesting, and clears the buffered state.
    pub async fn commit(&self) -> io::Result<()> {
        let actions = {
            let mut st = self.state.lock().await;
            log::debug!(
                "Transaction: force commit ({} action(s))",
                st.actions.len()
            );
            let actions = std::mem::take(&mut st.actions);
            st.tx_stack.clear();
            st.checkpoints.clear();
            st.overlay = InMemorySys::default();
            st.deleted.clear();
            st.cwd = None;
            actions
        };

        flush_to_real(&*self.real, &actions).await
    }

    /// Records a checkpoint at the current position in the action log.
    pub async fn checkpoint(&self) -> Checkpoint {
        let mut st = self.state.lock().await;
        let id = Checkpoint(st.next_checkpoint);
        st.next_checkpoint += 1;
        let len = st.actions.len();
        st.checkpoints.push((id, len));
        log::debug!("Transaction: checkpoint {:?} at action {}", id, len);
        id
    }

    /// Rolls back every action recorded after the given checkpoint.
    ///
    /// The checkpoint itself remains valid and can be rolled back to again.
    pub async fn rollback_to_checkpoint(
        &self,
        checkpoint: Checkpoint,
    ) -> io::Result<()> {
        let mut st = self.state.lock().await;
        let pos = st
            .checkpoints
            .iter()
            .position(|(c, _)| *c == checkpoint)
            .ok_or_else(|| {
                invalid("rollback_to_checkpoint: unknown checkpoint")
            })?;
        let len = st.checkpoints[pos].1;

        log::debug!(
            "Transaction: rollback to checkpoint {:?} (action {})",
            checkpoint,
            len
        );
        st.actions.truncate(len);
        // Keep checkpoints up to and including the target; drop later ones.
        st.checkpoints.truncate(pos + 1);
        // Drop any transactions that were opened after this checkpoint.
        st.tx_stack.retain(|start| *start <= len);
        rebuild(&mut st, &*self.real).await;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FsSys trait implementations
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsReadAsync for TransactionSys<S> {
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        let cp = self.resolve(path).await;
        let st = self.state.lock().await;

        if let Ok(data) = st.overlay.fs_read(&cp) {
            return Ok(Cow::Owned(data.into_owned()));
        }
        if is_shadowed(&st.deleted, &cp) {
            return Err(not_found(&cp));
        }

        let content = self.real.base_fs_read_async(&cp).await?;

        // Mirror the freshly fetched content into the overlay so subsequent
        // reads and mutations are observable. This is *not* logged as an
        // action: it only caches existing real state.
        if let Some(dir) = cp.parent() {
            let _ = st.overlay.fs_create_dir_all(dir);
        }
        let _ = st.overlay.fs_write(&cp, &content);

        Ok(content)
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsWriteAsync for TransactionSys<S> {
    async fn base_fs_write_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        let cp = self.resolve(path).await;
        log::info!("Transaction: write to {}", cp.display());
        let action = Action::Write {
            path: cp,
            data: data.to_vec(),
        };
        self.record(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsAppendAsync for TransactionSys<S> {
    async fn base_fs_append_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        let cp = self.resolve(path).await;
        log::info!("Transaction: append to {}", cp.display());
        let action = Action::Append {
            path: cp,
            data: data.to_vec(),
        };
        self.record(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsCreateDirAsync for TransactionSys<S> {
    async fn base_fs_create_dir_async(
        &self,
        path: &Path,
        options: &CreateDirOptions,
    ) -> io::Result<()> {
        let cp = self.resolve(path).await;
        log::info!("Transaction: create directory {}", cp.display());
        let action = Action::CreateDir {
            path: cp,
            options: *options,
        };
        self.record(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsRemoveFileAsync for TransactionSys<S> {
    async fn base_fs_remove_file_async(&self, path: &Path) -> io::Result<()> {
        let cp = self.resolve(path).await;
        log::info!("Transaction: remove file {}", cp.display());
        let action = Action::RemoveFile { path: cp };
        self.record(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsRemoveDirAsync for TransactionSys<S> {
    async fn base_fs_remove_dir_async(&self, path: &Path) -> io::Result<()> {
        let cp = self.resolve(path).await;
        log::info!("Transaction: remove directory {}", cp.display());
        let action = Action::RemoveDir { path: cp };
        self.record(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsRemoveDirAllAsync for TransactionSys<S> {
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        let cp = self.resolve(path).await;
        log::info!(
            "Transaction: remove directory and all of its contents {}",
            cp.display()
        );
        let action = Action::RemoveDirAll { path: cp };
        self.record(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsRenameAsync for TransactionSys<S> {
    async fn base_fs_rename_async(
        &self,
        from: &Path,
        to: &Path,
    ) -> io::Result<()> {
        let from = self.resolve(from).await;
        let to = self.resolve(to).await;
        log::info!(
            "Transaction: rename {} -> {}",
            from.display(),
            to.display()
        );
        let action = Action::Rename { from, to };
        self.record(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsCopyAsync for TransactionSys<S> {
    async fn base_fs_copy_async(
        &self,
        from: &Path,
        to: &Path,
    ) -> io::Result<u64> {
        let from = self.resolve(from).await;
        let to = self.resolve(to).await;
        log::info!("Transaction: copy {} -> {}", from.display(), to.display());
        let action = Action::Copy { from, to };
        self.record_with_result(action).await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsMetadataAsync for TransactionSys<S> {
    type Metadata = BoxedFsMetadataValue;

    async fn base_fs_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        let cp = self.resolve(path).await;
        let st = self.state.lock().await;

        if st.overlay.fs_exists_no_err(&cp)
            && let Ok(metadata) = st.overlay.fs_metadata(&cp)
        {
            return Ok(BoxedFsMetadataValue::new(metadata));
        }
        if is_shadowed(&st.deleted, &cp) {
            return Err(not_found(&cp));
        }

        let result = self
            .real
            .base_fs_metadata_async(&cp)
            .await
            .map(BoxedFsMetadataValue::new)?;

        // Materialise directories into the overlay so directory listings can
        // merge dry-run additions with the real tree.
        if result.file_type() == FileType::Dir {
            let _ = st.overlay.fs_create_dir_all(&cp);
        }

        Ok(result)
    }

    async fn base_fs_symlink_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        let cp = self.resolve(path).await;
        let st = self.state.lock().await;

        if st.overlay.fs_exists_no_err(&cp)
            && let Ok(metadata) = st.overlay.fs_symlink_metadata(&cp)
        {
            return Ok(BoxedFsMetadataValue::new(metadata));
        }
        if is_shadowed(&st.deleted, &cp) {
            return Err(not_found(&cp));
        }

        self.real
            .base_fs_symlink_metadata_async(&cp)
            .await
            .map(BoxedFsMetadataValue::new)
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseFsReadDirAsync for TransactionSys<S> {
    async fn base_fs_read_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        let cp = self.resolve(path).await;
        let st = self.state.lock().await;

        let overlay_exists = st.overlay.fs_exists_no_err(&cp);
        if !overlay_exists && is_shadowed(&st.deleted, &cp) {
            return Err(not_found(&cp));
        }

        let mut entries: BTreeSet<PathBuf> = BTreeSet::new();

        if overlay_exists {
            if let Ok(es) = st.overlay.base_fs_read_dir_async(&cp).await {
                entries.extend(es);
            }
        }

        match self.real.base_fs_read_dir_async(&cp).await {
            Ok(es) => {
                for entry in es {
                    if !is_shadowed(&st.deleted, &entry) {
                        entries.insert(entry);
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if !overlay_exists && entries.is_empty() {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }

        Ok(entries.into_iter().collect())
    }
}

// ---------------------------------------------------------------------------
// ProcSys trait implementations
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl<S: GeneratorSys> EnvCurrentDirAsync for TransactionSys<S> {
    async fn env_current_dir_async(&self) -> io::Result<PathBuf> {
        if let Some(cwd) = self.state.lock().await.cwd.clone() {
            return Ok(cwd);
        }
        self.real.env_current_dir_async().await
    }
}

#[async_trait::async_trait]
impl<S: GeneratorSys> BaseEnvSetCurrentDirAsync for TransactionSys<S> {
    async fn base_env_set_current_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        let cp = self.resolve(path).await;
        log::info!("Transaction: set current dir {}", cp.display());
        let mut st = self.state.lock().await;
        st.cwd = Some(cp.clone());
        st.actions.push(Action::SetCurrentDir { path: cp });
        Ok(())
    }
}

impl<S: GeneratorSys> EnvVars for TransactionSys<S> {
    fn env_vars(&self) -> std::env::Vars {
        self.real.env_vars()
    }

    fn env_vars_os(&self) -> std::env::VarsOs {
        self.real.env_vars_os()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl<S: GeneratorSys> TransactionSys<S> {
    /// Resolves `path` to an absolute, normalized path using the transaction's
    /// *logical* working directory (the buffered [`set_current_dir`] override,
    /// if any, otherwise the real working directory).
    ///
    /// Because `set_current_dir` is buffered and never touches the real process
    /// cwd until commit, relative paths must be anchored here rather than left
    /// for the real system to resolve. Doing so guarantees that a relative path
    /// and its absolute equivalent address the same overlay entry, and that
    /// fall-through reads/metadata hit the same real file a plain system would.
    /// In other words, callers observe identical path semantics whether or not
    /// the transactional overlay is present.
    ///
    /// [`set_current_dir`]: BaseEnvSetCurrentDirAsync::base_env_set_current_dir_async
    async fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return clean(path);
        }
        let override_cwd = self.state.lock().await.cwd.clone();
        let base = match override_cwd {
            Some(base) => base,
            None => match self.real.env_current_dir_async().await {
                Ok(cwd) => cwd,
                // If even the real cwd is unavailable, fall back to normalizing
                // the relative path as-is and let the real system resolve it.
                Err(_) => return clean(path),
            },
        };
        clean(&base.join(path))
    }

    /// Applies a mutating action to the overlay and, on success, records it in
    /// the action log.
    async fn record(&self, action: Action) -> io::Result<()> {
        let mut guard = self.state.lock().await;
        let st: &mut State = &mut guard;
        apply_overlay(&st.overlay, &mut st.deleted, &*self.real, &action)
            .await?;
        st.actions.push(action);
        Ok(())
    }

    /// Like [`Self::record`] but returns the number of bytes reported by the
    /// overlay effect (used by `copy`).
    async fn record_with_result(&self, action: Action) -> io::Result<u64> {
        let mut guard = self.state.lock().await;
        let st: &mut State = &mut guard;
        let result =
            apply_overlay(&st.overlay, &mut st.deleted, &*self.real, &action)
                .await?;
        st.actions.push(action);
        Ok(result)
    }
}

/// Rebuilds the in-memory overlay (and the deleted/cwd state) by replaying the
/// remaining action log from scratch.
async fn rebuild<S: GeneratorSys>(st: &mut State, real: &S) {
    st.overlay = InMemorySys::default();
    st.deleted.clear();

    let actions = std::mem::take(&mut st.actions);
    let mut cwd = None;
    for action in &actions {
        if let Action::SetCurrentDir { path } = action {
            cwd = Some(path.clone());
        }
        if let Err(err) =
            apply_overlay(&st.overlay, &mut st.deleted, real, action).await
        {
            log::warn!("Transaction: failed to replay action: {err}");
        }
    }
    st.actions = actions;
    st.cwd = cwd;
}

/// Applies a single mutating action to the overlay, resolving any base content
/// from the real file system when necessary. Returns the number of bytes
/// written for copy actions (and `0` otherwise).
async fn apply_overlay<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &mut HashSet<PathBuf>,
    real: &S,
    action: &Action,
) -> io::Result<u64> {
    match action {
        Action::Write { path, data } => {
            if let Some(dir) = path.parent() {
                let _ = overlay.fs_create_dir_all(dir);
            }
            overlay.fs_write(path, data)?;
            deleted.remove(path);
            Ok(0)
        }
        Action::Append { path, data } => {
            let mut existing =
                read_existing(overlay, deleted, real, path).await?;
            existing.extend_from_slice(data);
            if let Some(dir) = path.parent() {
                let _ = overlay.fs_create_dir_all(dir);
            }
            overlay.fs_write(path, &existing)?;
            deleted.remove(path);
            Ok(0)
        }
        Action::CreateDir { path, options } => {
            if options.recursive {
                overlay.fs_create_dir_all(path)?;
            } else {
                if let Some(dir) = path.parent() {
                    materialize_dir_if_real(overlay, deleted, real, dir).await;
                }
                overlay.base_fs_create_dir(path, options)?;
            }
            deleted.remove(path);
            Ok(0)
        }
        Action::RemoveFile { path } => {
            remove_path(overlay, deleted, real, path, false).await?;
            Ok(0)
        }
        Action::RemoveDir { path } => {
            remove_path(overlay, deleted, real, path, false).await?;
            Ok(0)
        }
        Action::RemoveDirAll { path } => {
            remove_path(overlay, deleted, real, path, true).await?;
            Ok(0)
        }
        Action::Rename { from, to } => {
            rename_path(overlay, deleted, real, from, to).await?;
            Ok(0)
        }
        Action::Copy { from, to } => {
            let data = read_required(overlay, deleted, real, from).await?;
            if let Some(dir) = to.parent() {
                let _ = overlay.fs_create_dir_all(dir);
            }
            overlay.fs_write(to, &data)?;
            deleted.remove(to);
            Ok(data.len() as u64)
        }
        // The working directory is tracked at the [`State`] level, not in the
        // overlay, so there is nothing to apply here.
        Action::SetCurrentDir { .. } => Ok(0),
    }
}

/// Returns `true` if `path` (or any of its ancestors) has been removed within
/// the active transaction(s).
fn is_shadowed(deleted: &HashSet<PathBuf>, path: &Path) -> bool {
    path.ancestors().any(|ancestor| deleted.contains(ancestor))
}

/// Reads the current contents of `path`, returning an empty buffer if the path
/// does not exist anywhere.
async fn read_existing<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &HashSet<PathBuf>,
    real: &S,
    path: &Path,
) -> io::Result<Vec<u8>> {
    if let Ok(data) = overlay.fs_read(path) {
        return Ok(data.into_owned());
    }
    if is_shadowed(deleted, path) {
        return Ok(Vec::new());
    }
    match real.base_fs_read_async(path).await {
        Ok(data) => Ok(data.into_owned()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

/// Reads the current contents of `path`, erroring if the path does not exist.
async fn read_required<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &HashSet<PathBuf>,
    real: &S,
    path: &Path,
) -> io::Result<Vec<u8>> {
    if let Ok(data) = overlay.fs_read(path) {
        return Ok(data.into_owned());
    }
    if is_shadowed(deleted, path) {
        return Err(not_found(path));
    }
    Ok(real.base_fs_read_async(path).await?.into_owned())
}

/// Returns whether `path` currently exists (in the overlay or, falling back,
/// the real file system) while honouring whiteouts.
async fn path_exists<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &HashSet<PathBuf>,
    real: &S,
    path: &Path,
) -> io::Result<bool> {
    if overlay.fs_exists_no_err(path) {
        return Ok(true);
    }
    if is_shadowed(deleted, path) {
        return Ok(false);
    }
    real.fs_exists_async(path).await
}

/// Returns whether `path` resolves to a directory, erroring if it is missing.
async fn is_dir_resolved<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &HashSet<PathBuf>,
    real: &S,
    path: &Path,
) -> io::Result<bool> {
    if overlay.fs_exists_no_err(path) {
        return Ok(overlay.fs_is_dir_no_err(path));
    }
    if is_shadowed(deleted, path) {
        return Err(not_found(path));
    }
    Ok(real.base_fs_metadata_async(path).await?.file_type() == FileType::Dir)
}

/// Mirrors a directory that only exists on the real file system into the
/// overlay so that subsequent non-recursive operations succeed.
async fn materialize_dir_if_real<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &HashSet<PathBuf>,
    real: &S,
    path: &Path,
) {
    if overlay.fs_exists_no_err(path) || is_shadowed(deleted, path) {
        return;
    }
    if real.fs_is_dir_no_err_async(path).await {
        let _ = overlay.fs_create_dir_all(path);
    }
}

/// Removes a path from the overlay and records a whiteout so the real path is
/// shadowed.
async fn remove_path<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &mut HashSet<PathBuf>,
    real: &S,
    path: &Path,
    recursive: bool,
) -> io::Result<()> {
    if !path_exists(overlay, deleted, real, path).await? {
        return Err(not_found(path));
    }

    if overlay.fs_exists_no_err(path) {
        if recursive {
            let _ = overlay.base_fs_remove_dir_all(path);
        } else if overlay.fs_is_dir_no_err(path) {
            let _ = overlay.base_fs_remove_dir(path);
        } else {
            let _ = overlay.fs_remove_file(path);
        }
    }

    deleted.insert(path.to_path_buf());
    Ok(())
}

/// Lists the immediate children of `dir`, merging overlay and real entries and
/// honouring whiteouts.
async fn read_dir_merged<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &HashSet<PathBuf>,
    real: &S,
    dir: &Path,
) -> io::Result<Vec<PathBuf>> {
    let mut entries: BTreeSet<PathBuf> = BTreeSet::new();

    if overlay.fs_exists_no_err(dir)
        && let Ok(es) = overlay.base_fs_read_dir_async(dir).await
    {
        entries.extend(es);
    }

    match real.base_fs_read_dir_async(dir).await {
        Ok(es) => {
            for entry in es {
                if !is_shadowed(deleted, &entry) {
                    entries.insert(entry);
                }
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }

    Ok(entries.into_iter().collect())
}

/// Collects every (non-directory) file beneath `dir`, merging overlay and real
/// state.
async fn collect_files<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &HashSet<PathBuf>,
    real: &S,
    dir: &Path,
) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = read_dir_merged(overlay, deleted, real, &current).await?;
        for entry in entries {
            if is_dir_resolved(overlay, deleted, real, &entry)
                .await
                .unwrap_or(false)
            {
                stack.push(entry);
            } else {
                files.push(entry);
            }
        }
    }

    Ok(files)
}

/// Moves `from` to `to` within the overlay, recursively for directories.
async fn rename_path<S: GeneratorSys>(
    overlay: &InMemorySys,
    deleted: &mut HashSet<PathBuf>,
    real: &S,
    from: &Path,
    to: &Path,
) -> io::Result<()> {
    let is_dir = is_dir_resolved(overlay, deleted, real, from).await?;

    if is_dir {
        let files = collect_files(overlay, deleted, real, from).await?;
        for file in files {
            let rel = file.strip_prefix(from).unwrap_or(&file);
            let dest = to.join(rel);
            let data = read_required(overlay, deleted, real, &file).await?;
            if let Some(dir) = dest.parent() {
                let _ = overlay.fs_create_dir_all(dir);
            }
            overlay.fs_write(&dest, &data)?;
            deleted.remove(&dest);
        }
        // Ensure the destination directory exists even when empty.
        let _ = overlay.fs_create_dir_all(to);
        deleted.remove(to);

        if overlay.fs_exists_no_err(from) {
            let _ = overlay.base_fs_remove_dir_all(from);
        }
        deleted.insert(from.to_path_buf());
    } else {
        let data = read_required(overlay, deleted, real, from).await?;
        if let Some(dir) = to.parent() {
            let _ = overlay.fs_create_dir_all(dir);
        }
        overlay.fs_write(to, &data)?;
        deleted.remove(to);

        if overlay.fs_exists_no_err(from) {
            let _ = overlay.fs_remove_file(from);
        }
        deleted.insert(from.to_path_buf());
    }

    Ok(())
}

/// Replays the buffered action log against the real file system.
async fn flush_to_real<S: GeneratorSys>(
    real: &S,
    actions: &[Action],
) -> io::Result<()> {
    for action in actions {
        match action {
            Action::Write { path, data } => {
                ensure_real_parent(real, path).await?;
                real.base_fs_write_async(path, data).await?;
            }
            Action::Append { path, data } => {
                ensure_real_parent(real, path).await?;
                real.base_fs_append_async(path, data).await?;
            }
            Action::CreateDir { path, options } => {
                match real.base_fs_create_dir_async(path, options).await {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(err) => return Err(err),
                }
            }
            Action::RemoveFile { path } => {
                ignore_not_found(real.base_fs_remove_file_async(path).await)?;
            }
            Action::RemoveDir { path } => {
                ignore_not_found(real.base_fs_remove_dir_async(path).await)?;
            }
            Action::RemoveDirAll { path } => {
                ignore_not_found(
                    real.base_fs_remove_dir_all_async(path).await,
                )?;
            }
            Action::Rename { from, to } => {
                ensure_real_parent(real, to).await?;
                real.base_fs_rename_async(from, to).await?;
            }
            Action::Copy { from, to } => {
                ensure_real_parent(real, to).await?;
                real.base_fs_copy_async(from, to).await?;
            }
            Action::SetCurrentDir { path } => {
                real.base_env_set_current_dir_async(path).await?;
            }
        }
    }
    Ok(())
}

async fn ensure_real_parent<S: GeneratorSys>(
    real: &S,
    path: &Path,
) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        real.fs_create_dir_all_async(dir).await?;
    }
    Ok(())
}

fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn clean(path: &Path) -> PathBuf {
    path.clean()
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("path not found: {}", path.display()),
    )
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsString;
    use std::sync::OnceLock;

    use system_traits::{
        EnvSetCurrentDirAsync, FsAppendAsync, FsCopyAsync, FsCreateDirAllAsync,
        FsCreateDirAsync, FsMetadataAsync, FsReadAsync, FsReadDirAsync,
        FsRemoveDirAllAsync, FsRemoveDirAsync, FsRemoveFileAsync,
        FsRenameAsync, FsWriteAsync,
    };
    use tempfile::{TempDir, tempdir};

    use super::*;

    /// Creates a fresh temporary directory whose lifetime is tied to the
    /// returned [`TempDir`] handle (cleaned up automatically on drop).
    fn temp() -> TempDir {
        tempdir().expect("failed to create temp dir")
    }

    /// Collects the file names of a directory listing into a sorted set.
    fn names(entries: &[PathBuf]) -> BTreeSet<String> {
        entries
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    /// Serializes every test that mutates or observes the *process-global*
    /// current working directory, so they cannot race each other.
    fn cwd_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Holds the [`cwd_lock`] and the original working directory, restoring the
    /// latter on drop. Keeps process-global cwd mutations isolated to the test
    /// that performs them, even if the test panics.
    struct CwdGuard {
        original: PathBuf,
        _lock: tokio::sync::MutexGuard<'static, ()>,
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    /// Acquires the global cwd lock, points the real process cwd at `dir`, and
    /// returns a guard that restores the previous cwd (and releases the lock)
    /// when dropped. The returned canonical cwd should be used for assertions,
    /// since the OS may canonicalize symlinked temp paths.
    async fn enter_cwd(dir: &Path) -> (CwdGuard, PathBuf) {
        let lock = cwd_lock().lock().await;
        let original = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(dir).expect("set current dir");
        let canonical = std::env::current_dir().expect("read current dir");
        (
            CwdGuard {
                original,
                _lock: lock,
            },
            canonical,
        )
    }

    // -- individual filesystem actions --------------------------------------

    #[tokio::test]
    async fn write_is_buffered_until_commit() {
        let dir = temp();
        let path = dir.path().join("file.txt");
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_write_async(&path, b"hello").await.unwrap();

        // Visible through the overlay...
        assert_eq!(
            sys.fs_read_async(&path).await.unwrap().into_owned(),
            b"hello"
        );
        // ...but not yet on the real file system.
        assert!(!path.exists());
        assert_eq!(sys.pending_actions().await, 1);

        sys.commit().await.unwrap();

        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        assert_eq!(sys.pending_actions().await, 0);
    }

    #[tokio::test]
    async fn append_creates_and_extends() {
        let dir = temp();
        let existing = dir.path().join("existing.txt");
        let fresh = dir.path().join("fresh.txt");
        std::fs::write(&existing, b"base").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        // Append to a file that only exists on the real fs.
        sys.fs_append_async(&existing, b"-more").await.unwrap();
        assert_eq!(
            sys.fs_read_async(&existing).await.unwrap().into_owned(),
            b"base-more"
        );
        // The real file is untouched until commit.
        assert_eq!(std::fs::read(&existing).unwrap(), b"base");

        // Append to a brand new file (creates it).
        sys.fs_append_async(&fresh, b"abc").await.unwrap();
        sys.fs_append_async(&fresh, b"def").await.unwrap();
        assert_eq!(
            sys.fs_read_async(&fresh).await.unwrap().into_owned(),
            b"abcdef"
        );

        sys.commit().await.unwrap();
        assert_eq!(std::fs::read(&existing).unwrap(), b"base-more");
        assert_eq!(std::fs::read(&fresh).unwrap(), b"abcdef");
    }

    #[tokio::test]
    async fn create_dir_and_create_dir_all() {
        let dir = temp();
        let nested = dir.path().join("a/b/c");
        let single = dir.path().join("single");
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_create_dir_all_async(&nested).await.unwrap();
        assert!(sys.fs_is_dir_async(&nested).await.unwrap());
        assert!(!nested.exists());

        // Non-recursive create directly under an existing (real) directory.
        sys.fs_create_dir_async(&single, &CreateDirOptions::default())
            .await
            .unwrap();
        assert!(sys.fs_is_dir_async(&single).await.unwrap());

        sys.commit().await.unwrap();
        assert!(nested.is_dir());
        assert!(single.is_dir());
    }

    #[tokio::test]
    async fn remove_file_shadows_real_until_commit() {
        let dir = temp();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"data").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_remove_file_async(&path).await.unwrap();

        // Shadowed in the overlay even though it still exists on disk.
        assert!(!sys.fs_exists_async(&path).await.unwrap());
        assert!(path.exists());

        sys.commit().await.unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn remove_empty_dir() {
        let dir = temp();
        let target = dir.path().join("empty");
        std::fs::create_dir(&target).unwrap();
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_remove_dir_async(&target).await.unwrap();
        assert!(!sys.fs_exists_async(&target).await.unwrap());
        assert!(target.exists());

        sys.commit().await.unwrap();
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn remove_dir_all_with_contents() {
        let dir = temp();
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        std::fs::write(tree.join("sub/file.txt"), b"x").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_remove_dir_all_async(&tree).await.unwrap();

        // The whole subtree is shadowed.
        assert!(!sys.fs_exists_async(&tree).await.unwrap());
        assert!(
            !sys.fs_exists_async(&tree.join("sub/file.txt"))
                .await
                .unwrap()
        );
        assert!(tree.exists());

        sys.commit().await.unwrap();
        assert!(!tree.exists());
    }

    #[tokio::test]
    async fn copy_file() {
        let dir = temp();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("nested/to.txt");
        std::fs::write(&from, b"payload").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        let copied = sys.fs_copy_async(&from, &to).await.unwrap();
        assert_eq!(copied, b"payload".len() as u64);
        assert_eq!(
            sys.fs_read_async(&to).await.unwrap().into_owned(),
            b"payload"
        );
        // Source still exists, destination not yet on disk.
        assert!(from.exists());
        assert!(!to.exists());

        sys.commit().await.unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"payload");
        assert_eq!(std::fs::read(&from).unwrap(), b"payload");
    }

    #[tokio::test]
    async fn rename_file() {
        let dir = temp();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        std::fs::write(&from, b"content").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_rename_async(&from, &to).await.unwrap();
        assert_eq!(
            sys.fs_read_async(&to).await.unwrap().into_owned(),
            b"content"
        );
        assert!(!sys.fs_exists_async(&from).await.unwrap());

        sys.commit().await.unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"content");
    }

    #[tokio::test]
    async fn rename_directory_recursively() {
        let dir = temp();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_create_dir_all_async(&src.join("sub")).await.unwrap();
        sys.fs_write_async(&src.join("top.txt"), b"top")
            .await
            .unwrap();
        sys.fs_write_async(&src.join("sub/inner.txt"), b"inner")
            .await
            .unwrap();

        sys.fs_rename_async(&src, &dst).await.unwrap();

        assert!(!sys.fs_exists_async(&src).await.unwrap());
        assert_eq!(
            sys.fs_read_async(&dst.join("top.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"top"
        );
        assert_eq!(
            sys.fs_read_async(&dst.join("sub/inner.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"inner"
        );

        sys.commit().await.unwrap();
        assert!(!src.exists());
        assert_eq!(std::fs::read(dst.join("top.txt")).unwrap(), b"top");
        assert_eq!(std::fs::read(dst.join("sub/inner.txt")).unwrap(), b"inner");
    }

    #[tokio::test]
    async fn read_dir_merges_overlay_and_real() {
        let dir = temp();
        std::fs::write(dir.path().join("keep.txt"), b"k").unwrap();
        std::fs::write(dir.path().join("drop.txt"), b"d").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        // Add a buffered file and remove a real one.
        sys.fs_write_async(&dir.path().join("added.txt"), b"a")
            .await
            .unwrap();
        sys.fs_remove_file_async(&dir.path().join("drop.txt"))
            .await
            .unwrap();

        let entries = sys.fs_read_dir_async(dir.path()).await.unwrap();
        assert_eq!(
            names(&entries),
            BTreeSet::from(["keep.txt".to_string(), "added.txt".to_string()])
        );
    }

    #[tokio::test]
    async fn metadata_reflects_overlay_and_real() {
        let dir = temp();
        let real_file = dir.path().join("real.txt");
        let real_dir = dir.path().join("real_dir");
        std::fs::write(&real_file, b"x").unwrap();
        std::fs::create_dir(&real_dir).unwrap();
        let sys = TransactionSys::new(RealSys::default());

        // Real entries are observable through metadata.
        assert!(sys.fs_is_file_async(&real_file).await.unwrap());
        assert!(sys.fs_is_dir_async(&real_dir).await.unwrap());
        assert!(
            !sys.fs_exists_async(&dir.path().join("missing"))
                .await
                .unwrap()
        );

        // Buffered entries too.
        let buffered = dir.path().join("buffered.txt");
        sys.fs_write_async(&buffered, b"y").await.unwrap();
        assert!(sys.fs_is_file_async(&buffered).await.unwrap());
        assert!(sys.fs_exists_async(&buffered).await.unwrap());
    }

    #[tokio::test]
    async fn set_current_dir_is_buffered_and_reverts() {
        let dir = temp();
        // This test observes the real cwd, so serialize it against the tests
        // that mutate the process-global cwd.
        let _cwd_lock = cwd_lock().lock().await;
        let sys = TransactionSys::new(RealSys::default());
        let real_cwd = sys.env_current_dir_async().await.unwrap();

        sys.begin_transaction().await;
        sys.env_set_current_dir_async(dir.path()).await.unwrap();
        assert_eq!(sys.env_current_dir_async().await.unwrap(), dir.path());

        // Rolling back drops the override (without touching the real cwd).
        sys.rollback_transaction().await.unwrap();
        assert_eq!(sys.env_current_dir_async().await.unwrap(), real_cwd);
    }

    #[tokio::test]
    async fn env_vars_delegate_to_real() {
        // SAFETY: single-threaded test setup of a unique key.
        let key = "OMNI_TX_TEST_ENV_VAR";
        unsafe { std::env::set_var(key, "value") };
        let sys = TransactionSys::new(RealSys::default());

        let found = sys.env_vars().any(|(k, v)| k == key && v == "value");
        assert!(found);

        let found_os = sys.env_vars_os().any(|(k, _)| k == OsString::from(key));
        assert!(found_os);

        unsafe { std::env::remove_var(key) };
    }

    // -- path resolution ----------------------------------------------------
    //
    // These tests pin down the guarantee that paths behave the same whether or
    // not the transactional overlay is present. They deliberately stay at the
    // buffered (overlay) layer and never commit a `set_current_dir`, because
    // committing one would replay it onto the real process and mutate the
    // shared, process-global working directory (racy under parallel tests).

    #[tokio::test]
    async fn relative_paths_resolve_against_real_cwd_without_override() {
        let dir = temp();
        // Isolate the process-global cwd to a temp directory for this test.
        let (_cwd, real_cwd) = enter_cwd(dir.path()).await;
        let sys = TransactionSys::new(RealSys::default());

        // With no `set_current_dir` override, a relative path must resolve
        // against the real working directory, exactly as the underlying system
        // would. A buffered write is therefore addressable through both the
        // relative path and its cwd-joined absolute equivalent.
        sys.fs_write_async(Path::new("rel_probe.txt"), b"probe")
            .await
            .unwrap();

        let abs = real_cwd.join("rel_probe.txt");
        assert_eq!(
            sys.fs_read_async(&abs).await.unwrap().into_owned(),
            b"probe"
        );
        assert_eq!(
            sys.fs_read_async(Path::new("rel_probe.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"probe"
        );
        // Still buffered: nothing was written to the temp working directory.
        assert!(!abs.exists());
    }

    #[tokio::test]
    async fn relative_paths_resolve_against_set_current_dir() {
        let dir = temp();
        let sys = TransactionSys::new(RealSys::default());
        sys.env_set_current_dir_async(dir.path()).await.unwrap();

        // A relative write lands under the (buffered) logical cwd.
        sys.fs_write_async(Path::new("rel.txt"), b"hi")
            .await
            .unwrap();

        // Readable through the relative path...
        assert_eq!(
            sys.fs_read_async(Path::new("rel.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"hi"
        );
        // ...and through the equivalent absolute path.
        assert_eq!(
            sys.fs_read_async(&dir.path().join("rel.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"hi"
        );
    }

    #[tokio::test]
    async fn relative_read_falls_through_to_real_under_set_current_dir() {
        let dir = temp();
        std::fs::write(dir.path().join("on_disk.txt"), b"disk").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        sys.env_set_current_dir_async(dir.path()).await.unwrap();

        // A relative read must fall through to the real file under the logical
        // cwd, just as a plain system (which would have actually changed the
        // process cwd) resolves it.
        assert_eq!(
            sys.fs_read_async(Path::new("on_disk.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"disk"
        );
        assert!(
            sys.fs_is_file_async(Path::new("on_disk.txt"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn relative_and_absolute_address_same_overlay_entry() {
        let dir = temp();
        let sys = TransactionSys::new(RealSys::default());
        sys.env_set_current_dir_async(dir.path()).await.unwrap();

        sys.fs_write_async(Path::new("shared.txt"), b"v")
            .await
            .unwrap();

        // Removing via the absolute equivalent removes the same entry the
        // relative write created.
        sys.fs_remove_file_async(&dir.path().join("shared.txt"))
            .await
            .unwrap();
        assert!(!sys.fs_exists_async(Path::new("shared.txt")).await.unwrap());
        assert!(
            !sys.fs_exists_async(&dir.path().join("shared.txt"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn dotted_relative_components_are_normalized() {
        let dir = temp();
        let sys = TransactionSys::new(RealSys::default());
        sys.env_set_current_dir_async(dir.path()).await.unwrap();

        // `./a/../b.txt` must normalize to `b.txt` under the logical cwd.
        sys.fs_write_async(Path::new("./a/../b.txt"), b"norm")
            .await
            .unwrap();
        assert_eq!(
            sys.fs_read_async(Path::new("b.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"norm"
        );
        assert_eq!(
            sys.fs_read_async(&dir.path().join("b.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"norm"
        );
    }

    #[tokio::test]
    async fn changing_set_current_dir_redirects_relative_paths() {
        let a = temp();
        let b = temp();
        let sys = TransactionSys::new(RealSys::default());

        sys.env_set_current_dir_async(a.path()).await.unwrap();
        sys.fs_write_async(Path::new("note.txt"), b"in-a")
            .await
            .unwrap();

        sys.env_set_current_dir_async(b.path()).await.unwrap();
        sys.fs_write_async(Path::new("note.txt"), b"in-b")
            .await
            .unwrap();

        // Each relative write landed under the cwd in effect at the time.
        assert_eq!(
            sys.fs_read_async(&a.path().join("note.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"in-a"
        );
        assert_eq!(
            sys.fs_read_async(&b.path().join("note.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"in-b"
        );
        // A bare relative path now resolves to the current cwd (b).
        assert_eq!(
            sys.fs_read_async(Path::new("note.txt"))
                .await
                .unwrap()
                .into_owned(),
            b"in-b"
        );
    }

    #[tokio::test]
    async fn relative_dir_operations_match_absolute() {
        let dir = temp();
        let sys = TransactionSys::new(RealSys::default());
        sys.env_set_current_dir_async(dir.path()).await.unwrap();

        sys.fs_create_dir_all_async(Path::new("nested/inner"))
            .await
            .unwrap();
        sys.fs_write_async(Path::new("nested/inner/f.txt"), b"x")
            .await
            .unwrap();

        // Listings via the relative and absolute paths agree.
        let rel = sys
            .fs_read_dir_async(Path::new("nested/inner"))
            .await
            .unwrap();
        let abs = sys
            .fs_read_dir_async(&dir.path().join("nested/inner"))
            .await
            .unwrap();
        assert_eq!(names(&rel), names(&abs));
        assert_eq!(names(&rel), BTreeSet::from(["f.txt".to_string()]));
    }

    #[tokio::test]
    async fn rollback_discards_relative_writes() {
        let dir = temp();
        let sys = TransactionSys::new(RealSys::default());
        sys.env_set_current_dir_async(dir.path()).await.unwrap();

        sys.begin_transaction().await;
        sys.fs_write_async(Path::new("temp.txt"), b"data")
            .await
            .unwrap();
        assert!(sys.fs_exists_async(Path::new("temp.txt")).await.unwrap());

        sys.rollback_transaction().await.unwrap();

        // The relative write is gone, but the logical cwd (set before the
        // transaction) survives, so relative resolution still works.
        assert!(!sys.fs_exists_async(Path::new("temp.txt")).await.unwrap());
        assert_eq!(sys.env_current_dir_async().await.unwrap(), dir.path());
    }

    // -- transaction control ------------------------------------------------

    #[tokio::test]
    async fn rollback_discards_writes() {
        let dir = temp();
        let path = dir.path().join("file.txt");
        let sys = TransactionSys::new(RealSys::default());

        sys.begin_transaction().await;
        sys.fs_write_async(&path, b"data").await.unwrap();
        assert!(sys.fs_exists_async(&path).await.unwrap());

        sys.rollback_transaction().await.unwrap();

        assert!(!sys.fs_exists_async(&path).await.unwrap());
        assert_eq!(sys.pending_actions().await, 0);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn nested_transactions_commit_at_top_level() {
        let dir = temp();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let sys = TransactionSys::new(RealSys::default());

        sys.begin_transaction().await;
        sys.fs_write_async(&a, b"a").await.unwrap();

        sys.begin_transaction().await;
        sys.fs_write_async(&b, b"b").await.unwrap();

        // Committing the inner transaction must not touch the real fs.
        sys.commit_transaction().await.unwrap();
        assert!(!a.exists());
        assert!(!b.exists());
        assert!(sys.in_transaction().await);

        // Committing the outer (top-level) transaction flushes everything.
        sys.commit_transaction().await.unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert!(!sys.in_transaction().await);
    }

    #[tokio::test]
    async fn nested_transaction_rollback_keeps_outer() {
        let dir = temp();
        let outer = dir.path().join("outer.txt");
        let inner = dir.path().join("inner.txt");
        let sys = TransactionSys::new(RealSys::default());

        sys.begin_transaction().await;
        sys.fs_write_async(&outer, b"o").await.unwrap();

        sys.begin_transaction().await;
        sys.fs_write_async(&inner, b"i").await.unwrap();
        sys.rollback_transaction().await.unwrap();

        // Inner work is gone, outer work survives.
        assert!(!sys.fs_exists_async(&inner).await.unwrap());
        assert!(sys.fs_exists_async(&outer).await.unwrap());

        sys.commit_transaction().await.unwrap();
        assert!(!inner.exists());
        assert_eq!(std::fs::read(&outer).unwrap(), b"o");
    }

    #[tokio::test]
    async fn commit_transaction_without_active_transaction_errors() {
        let sys = TransactionSys::new(RealSys::default());
        assert!(sys.commit_transaction().await.is_err());
        assert!(sys.rollback_transaction().await.is_err());
    }

    #[tokio::test]
    async fn checkpoint_rollback_undoes_later_actions() {
        let dir = temp();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let sys = TransactionSys::new(RealSys::default());

        sys.fs_write_async(&a, b"a").await.unwrap();
        let cp = sys.checkpoint().await;
        sys.fs_write_async(&b, b"b").await.unwrap();

        assert!(sys.fs_exists_async(&b).await.unwrap());

        sys.rollback_to_checkpoint(cp).await.unwrap();

        assert!(sys.fs_exists_async(&a).await.unwrap());
        assert!(!sys.fs_exists_async(&b).await.unwrap());
        assert_eq!(sys.pending_actions().await, 1);

        // The checkpoint remains usable for another rollback.
        sys.fs_write_async(&b, b"b2").await.unwrap();
        sys.rollback_to_checkpoint(cp).await.unwrap();
        assert!(!sys.fs_exists_async(&b).await.unwrap());
        assert_eq!(sys.pending_actions().await, 1);
    }

    #[tokio::test]
    async fn reads_fall_back_to_real_then_overlay() {
        let dir = temp();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"original").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        // First read mirrors the real content.
        assert_eq!(
            sys.fs_read_async(&path).await.unwrap().into_owned(),
            b"original"
        );

        // A buffered overwrite is visible through the overlay only.
        sys.fs_write_async(&path, b"updated").await.unwrap();
        assert_eq!(
            sys.fs_read_async(&path).await.unwrap().into_owned(),
            b"updated"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original");

        sys.commit().await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"updated");
    }

    /// Exercises every mutating action in a single transaction and verifies
    /// that committing the top-level transaction replays them all onto the
    /// real file system in order.
    #[tokio::test]
    async fn commit_replays_all_actions_to_real() {
        let dir = temp();
        let root = dir.path();
        let old = root.join("old.txt");
        std::fs::write(&old, b"old").unwrap();
        let sys = TransactionSys::new(RealSys::default());

        sys.begin_transaction().await;
        sys.fs_create_dir_all_async(&root.join("nested"))
            .await
            .unwrap();
        sys.fs_write_async(&root.join("nested/a.txt"), b"A")
            .await
            .unwrap();
        sys.fs_write_async(&root.join("b.txt"), b"B").await.unwrap();
        sys.fs_append_async(&root.join("b.txt"), b"B2")
            .await
            .unwrap();
        sys.fs_copy_async(&root.join("nested/a.txt"), &root.join("c.txt"))
            .await
            .unwrap();
        sys.fs_remove_file_async(&old).await.unwrap();
        sys.fs_rename_async(&root.join("b.txt"), &root.join("b2.txt"))
            .await
            .unwrap();

        // Nothing on disk yet (apart from the pre-existing file).
        assert!(old.exists());
        assert!(!root.join("nested/a.txt").exists());

        sys.commit_transaction().await.unwrap();

        assert!(!old.exists());
        assert_eq!(std::fs::read(root.join("nested/a.txt")).unwrap(), b"A");
        assert_eq!(std::fs::read(root.join("c.txt")).unwrap(), b"A");
        assert_eq!(std::fs::read(root.join("b2.txt")).unwrap(), b"BB2");
        assert!(!root.join("b.txt").exists());
    }

    /// End-to-end check that committing a buffered `set_current_dir` is
    /// replayed onto the real process and that relative writes recorded under
    /// it are flushed to the right place — i.e. the committed result is exactly
    /// what a plain system would have produced.
    #[tokio::test]
    async fn commit_replays_set_current_dir_and_relative_writes() {
        let dir = temp();
        // Committing a `set_current_dir` mutates the process-global cwd, so
        // isolate it. `enter_cwd` restores the previous cwd on drop.
        let (_cwd, root) = enter_cwd(dir.path()).await;
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();

        let sys = TransactionSys::new(RealSys::default());
        sys.begin_transaction().await;
        // Buffer a cwd change followed by a relative write.
        sys.env_set_current_dir_async(&work).await.unwrap();
        sys.fs_write_async(Path::new("out.txt"), b"committed")
            .await
            .unwrap();

        // Nothing on disk and the real cwd is unchanged until commit.
        assert!(!work.join("out.txt").exists());
        assert_eq!(std::env::current_dir().unwrap(), root);

        sys.commit_transaction().await.unwrap();

        // The relative write landed under the committed working directory...
        assert_eq!(std::fs::read(work.join("out.txt")).unwrap(), b"committed");
        // ...and the committed `set_current_dir` actually moved the process.
        assert_eq!(
            std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            std::fs::canonicalize(&work).unwrap()
        );
    }
}
