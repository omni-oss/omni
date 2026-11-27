use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use dir_walker::{
    DirEntry as _, DirWalker as _, FileType, impls::RealGlobDirWalker,
};
use enum_map::enum_map;
use globset::{Candidate, GlobSet};
use maps::Map;
use omni_command_config::CommandConfig;
use omni_glob::{GlobOptions, include_set};
use omni_hasher::{
    Hasher,
    impls::{DefaultHash, DefaultHasher},
    project_dir_hasher::{ProjectDirHasher, impls::RealDirHasher},
};
use omni_types::{OmniPath, Root, RootMap};
use omni_utils::path::{
    has_globs, path_safe, relpath, remove_globs, starts_with_path, topmost_dirs,
};
use system_traits::{
    FsCanonicalize, FsMetadata, FsMetadataAsync, auto_impl, impls::RealSys,
};
use tracing::Level;

use crate::{
    error::{Error, ErrorInner},
    identity_leaf::identity_leaf_v1,
    key_leaves::{cache_key_version_leaf, effective_task_leaf_v1},
};

/// The version of the whole cache key: its inputs and how they are combined.
///
/// A change to **any** key input or to the construction (a leaf, an order, a
/// collection rule, what the execution info contributes) bumps this constant in
/// the same change, besides any leaf version the change touches. Every bump is
/// a cold start; see `docs/patterns/cache-keys.md`.
pub const CACHE_KEY_VERSION: u32 = 1;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectConfig {
    pub output_files: bool,
    pub input_files: bool,
    pub digests: bool,
    pub cache_output_dirs: bool,
}

#[allow(clippy::too_many_arguments)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ProjectTaskInfo<'a> {
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub task_name: &'a str,
    pub task_exec: Option<&'a CommandConfig>,
    pub task_retry_exec: Option<&'a CommandConfig>,
    pub output_files: &'a [OmniPath],
    pub output_files_exclude: &'a [OmniPath],
    pub input_files: &'a [OmniPath],
    pub input_files_exclude: &'a [OmniPath],
    pub input_env_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
    pub args: &'a Map<String, serde_json::Value>,
    /// The digest of the task's effective definition (F06.3). Mandatory: it is
    /// in every key, whatever the input collection flags and
    /// `cache.key.defaults` say, so a constructor cannot leave it out.
    pub effective_digest: DefaultHash,
}

#[auto_impl]
pub trait CollectorSys:
    FsMetadata + FsMetadataAsync + FsCanonicalize + Clone + Send + Sync + 'static
{
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollectResult<'a> {
    pub task: ProjectTaskInfo<'a>,
    pub input_files: Option<Vec<OmniPath>>,
    pub output_files: Option<Vec<OmniPath>>,
    pub roots: RootMap<'a>,
    pub digest: Option<DefaultHash>,
    pub cache_output_dir: Option<PathBuf>,
}

struct HashInput<'a> {
    pub task_name: &'a str,
    pub task_exec: Option<&'a CommandConfig>,
    pub task_retry_exec: Option<&'a CommandConfig>,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub input_files: &'a [OmniPath],
    pub cached_output_files_glob: &'a [OmniPath],
    pub cached_output_files_exclude_glob: &'a [OmniPath],
    pub input_env_cache_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
    pub args: &'a Map<String, serde_json::Value>,
    pub effective_digest: DefaultHash,
}

struct Holder<'a> {
    output_files_globset: Option<Arc<GlobSet>>,
    output_files_exclude_globset: Option<Arc<GlobSet>>,
    output_files_glob: Vec<OmniPath>,
    output_files_exclude_glob: Vec<OmniPath>,
    resolved_output_files: Option<Vec<OmniPath>>,
    input_files_globset: Option<Arc<GlobSet>>,
    worktree_inputs_globset: Option<Arc<GlobSet>>,
    input_files_exclude_globset: Option<Arc<GlobSet>>,
    resolved_input_files: Option<Vec<OmniPath>>,
    /// Literal (glob-free) prefixes of this project's include paths. A walked
    /// file can only match one of the globsets if it lives under one of these
    /// directories, so it acts as a cheap rejection filter before the more
    /// expensive glob match.
    match_bases: Vec<PathBuf>,
    task: ProjectTaskInfo<'a>,
    roots: RootMap<'a>,
    digest: Option<DefaultHash>,
    cache_output_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Collector<'a, TSys: CollectorSys = RealSys> {
    ws_root_dir: &'a Path,
    sys: TSys,
    dir_hasher: RealDirHasher,
    /// The cache directory proper: the file-hash index lives here and it is
    /// kept out of every input walk, whatever partition entries are in.
    cache_dir: &'a Path,
    /// Where project output directories are kept. The cache directory itself
    /// unless a partition is chosen.
    output_base: &'a Path,
}

impl<'a, TSys: CollectorSys> Collector<'a, TSys> {
    pub fn new(ws_root_dir: &'a Path, cache_dir: &'a Path, sys: TSys) -> Self {
        let dir_hasher = RealDirHasher::builder()
            .workspace_root_dir(ws_root_dir.to_path_buf())
            .index_dir(cache_dir.to_path_buf())
            .build()
            .expect("failed to build hasher");

        Self {
            ws_root_dir,
            sys,
            dir_hasher,
            cache_dir,
            output_base: cache_dir,
        }
    }

    /// Keeps project output directories under `output_base` (a partition of
    /// the cache directory) while the hash index and the input-walk exclusion
    /// stay on the whole cache directory.
    pub fn with_output_base(mut self, output_base: &'a Path) -> Self {
        self.output_base = output_base;
        self
    }
}

impl<'a, TSys: CollectorSys> Collector<'a, TSys> {
    fn get_project_dir(&self, project_name: &str) -> PathBuf {
        let name = path_safe(project_name);

        self.output_base.join(name).join("output")
    }

    fn canonical_ws_root(&self) -> PathBuf {
        self.sys
            .fs_canonicalize(self.ws_root_dir)
            .unwrap_or_else(|_| self.ws_root_dir.to_path_buf())
    }

    // A followed symlink must never pull content from outside the workspace
    // root into an input hash, or the resulting cache key would depend on
    // machine-external, non-reproducible state. This predicate rejects any
    // symlink whose real target escapes the (canonicalized) workspace root.
    // Non-symlink entries take the fast path so the common case pays nothing.
    //
    // The walker does not run this predicate on the walk roots themselves
    // (only on entries discovered during descent), so escaping roots are
    // dropped separately before the walk starts.
    fn workspace_bound_predicate(
        &self,
        canonical_root: PathBuf,
        canonical_cache: PathBuf,
    ) -> impl Fn(&dir_walker::impls::IgnoreDirEntry) -> bool + Send + Sync + 'static
    {
        let sys = self.sys.clone();
        let cache_dir = canonical_cache
            .strip_prefix(&canonical_root)
            .map(|relative| self.ws_root_dir.join(relative))
            .unwrap_or_else(|_| self.cache_dir.to_path_buf());

        move |entry| {
            if entry.path().starts_with(&cache_dir)
                || entry.path().starts_with(&canonical_cache)
            {
                return false;
            }
            if !entry.path_is_symlink() {
                return true;
            }

            let link = entry.path();

            match sys.fs_canonicalize(link) {
                Ok(target) if target.starts_with(&canonical_cache) => false,
                Ok(target) if target.starts_with(&canonical_root) => true,
                Ok(target) => {
                    tracing::warn!(
                        link = ?link,
                        target = ?target,
                        "excluding symlink whose target escapes the workspace root"
                    );
                    false
                }
                // A target that cannot be resolved (e.g. a dangling link)
                // is left for the walker's own not-found handling to skip,
                // so containment rejection stays reserved for links that
                // resolve outside the root.
                Err(_) => true,
            }
        }
    }

    fn get_output_dir(
        &self,
        project_name: &str,
        hash: &str,
    ) -> Result<PathBuf, Error> {
        let proj_dir = self.get_project_dir(project_name);
        let output_dir = proj_dir.join(hash);

        Ok(output_dir)
    }

    async fn get_digest(
        &self,
        hash_input: &HashInput<'_>,
    ) -> Result<DefaultHash, Error> {
        let mut tree = self
            .dir_hasher
            .hash_tree::<DefaultHasher>(
                hash_input.project_name,
                hash_input.project_dir,
                hash_input.input_files,
            )
            .await
            .map_err(|e| ErrorInner::ProjectDirHasher(e.to_string()))?;

        let mut dep_hashes = hash_input.dependency_digests.to_vec();

        dep_hashes.sort();

        for dep_hash in dep_hashes {
            tree.insert(dep_hash);
        }

        if !hash_input.env_vars.is_empty() {
            let mut buff = vec![];
            let mut env_keys = hash_input
                .input_env_cache_keys
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>();

            env_keys.sort();

            for env_key in env_keys {
                let value = hash_input
                    .env_vars
                    .get(env_key)
                    .map(|s| s.as_str())
                    .unwrap_or("");

                buff.push(format!("{env_key}={value}"));
            }

            let env_vars = buff.join("\n");

            tree.insert(DefaultHasher::hash(env_vars.as_bytes()));
        }

        if !hash_input.args.is_empty() {
            // Sorted by key: map insertion order can vary between runs (e.g.
            // after a dictionary merge) and must not change the digest.
            let mut entries = hash_input.args.iter().collect::<Vec<_>>();
            entries.sort_by(|a, b| a.0.cmp(b.0));

            let mut buff = vec![];
            for (key, value) in entries {
                buff.push(format!("{key}={value:?}"));
            }

            let args = buff.join("\n");
            tree.insert(DefaultHasher::hash(args.as_bytes()));
        }

        if !hash_input.cached_output_files_glob.is_empty() {
            let mut sorted = hash_input
                .cached_output_files_glob
                .iter()
                .map(|p| p.unresolved_path())
                .collect::<Vec<_>>();

            sorted.sort();

            for path in sorted {
                tree.insert(DefaultHasher::hash(
                    path.to_string_lossy().as_bytes(),
                ));
            }
        }

        // Exclude patterns change which output files are cached, so they are
        // part of the key. They hash under a distinct prefix, and an empty
        // exclude set contributes nothing, so an include-only config hashes
        // exactly as before.
        if !hash_input.cached_output_files_exclude_glob.is_empty() {
            let mut sorted = hash_input
                .cached_output_files_exclude_glob
                .iter()
                .map(|p| p.unresolved_path())
                .collect::<Vec<_>>();

            sorted.sort();

            for path in sorted {
                tree.insert(DefaultHasher::hash(
                    format!("exclude:{}", path.to_string_lossy()).as_bytes(),
                ));
            }
        }

        if let Some(command) = hash_input.task_exec
            && let Some(command_str) = command_hash_input("command", command)
        {
            tree.insert(DefaultHasher::hash(command_str.as_bytes()));
        }
        if let Some(retry_command) = hash_input.task_retry_exec
            && let Some(retry_command_str) =
                command_hash_input("retry_command", retry_command)
        {
            tree.insert(DefaultHasher::hash(retry_command_str.as_bytes()));
        }

        tree.insert(
            identity_leaf_v1(hash_input.project_name, hash_input.task_name)
                .map_err(|e| ErrorInner::Unknown(eyre::eyre!(e)))?,
        );

        // The last two leaves, in this order, after the identity leaf: the key
        // version (any change to the key's inputs or construction bumps
        // `CACHE_KEY_VERSION`), then the effective task.
        tree.insert(cache_key_version_leaf(CACHE_KEY_VERSION));
        tree.insert(effective_task_leaf_v1(&hash_input.effective_digest));

        tree.commit();

        Ok(tree.root().expect("unable to get root"))
    }

    #[tracing::instrument(
            level = Level::DEBUG,
            skip_all,
            fields(project_tasks_count = project_tasks.len()),
        )]
    pub async fn collect(
        &self,
        project_tasks: &[ProjectTaskInfo<'a>],
        config: &CollectConfig,
    ) -> Result<Vec<CollectResult<'a>>, Error> {
        let mut to_process = Vec::with_capacity(project_tasks.len());

        tracing::trace!(?project_tasks, ?config, "begin_collect");

        let should_collect_input_files =
            config.input_files || config.digests || config.cache_output_dirs;

        let should_collect_output_files = config.output_files;

        // holds paths in input files and output files
        let mut includes = Vec::with_capacity(
            project_tasks
                .iter()
                .map(|i| {
                    let mut count = 0;

                    if should_collect_input_files {
                        count += i.input_files.len();
                    }

                    if should_collect_output_files {
                        count += i.output_files.len();
                    }

                    count
                })
                .sum(),
        );

        // Shared by all tasks and include/exclude patterns, but never retained
        // across collections where files or symlink targets may have changed.
        let mut alignment = PatternAlignment::new(self.ws_root_dir, &self.sys);

        for project in project_tasks {
            tracing::trace!(
                project_name = ?project.project_name,
                "processing_project"
            );
            let roots = enum_map! {
                Root::Workspace => &self.ws_root_dir,
                Root::Project => project.project_dir,
            };

            let mut match_bases = Vec::new();

            let mut output_files_globset = None;
            let mut output_files_exclude_globset = None;
            if should_collect_output_files {
                let mut output_patterns = Vec::new();

                populate_includes_and_patterns(
                    project.output_files,
                    project.project_dir,
                    &mut includes,
                    &mut match_bases,
                    &roots,
                    &mut output_patterns,
                    &mut alignment,
                )?;

                output_files_globset = Some(include_set(
                    &output_patterns,
                    GlobOptions::default(),
                )?);

                let mut output_exclude_patterns = Vec::new();
                resolve_exclude_patterns(
                    project.output_files_exclude,
                    project.project_dir,
                    &roots,
                    &mut output_exclude_patterns,
                    &mut alignment,
                )?;
                output_files_exclude_globset = Some(include_set(
                    &output_exclude_patterns,
                    GlobOptions::default(),
                )?);
            }

            let mut input_files_globset = None;
            let mut worktree_inputs_globset = None;
            let mut input_files_exclude_globset = None;

            // the output dirs are based on the hashes so need to be collected here
            if should_collect_input_files {
                let mut input_patterns = Vec::new();
                populate_includes_and_patterns(
                    project.input_files,
                    project.project_dir,
                    &mut includes,
                    &mut match_bases,
                    &roots,
                    &mut input_patterns,
                    &mut alignment,
                )?;

                input_files_globset =
                    Some(include_set(&input_patterns, GlobOptions::default())?);

                // Broad source globs do not pull machine-local policy into task
                // keys. A pattern naming that policy deliberately (literal or
                // extension glob) retains the ordinary explicit-input behavior.
                let worktree_patterns = input_patterns
                    .iter()
                    .filter(|pattern| {
                        Path::new(pattern)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                name.starts_with(
                                    omni_constants::WORKTREE_OMNI
                                        .trim_end_matches("{ext}"),
                                )
                            })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if !worktree_patterns.is_empty() {
                    worktree_inputs_globset = Some(include_set(
                        &worktree_patterns,
                        GlobOptions::default(),
                    )?);
                }

                let mut input_exclude_patterns = Vec::new();
                resolve_exclude_patterns(
                    project.input_files_exclude,
                    project.project_dir,
                    &roots,
                    &mut input_exclude_patterns,
                    &mut alignment,
                )?;
                input_files_exclude_globset = Some(include_set(
                    &input_exclude_patterns,
                    GlobOptions::default(),
                )?);
            }

            to_process.push(Holder {
                input_files_globset,
                worktree_inputs_globset,
                input_files_exclude_globset,
                output_files_globset,
                output_files_exclude_globset,
                output_files_glob: project.output_files.to_vec(),
                output_files_exclude_glob: project
                    .output_files_exclude
                    .to_vec(),
                match_bases,
                resolved_input_files: if should_collect_input_files {
                    // just in the collected input files will be the same as the
                    // original input files
                    Some(Vec::with_capacity(project.input_files.len()))
                } else {
                    None
                },
                resolved_output_files: if config.output_files {
                    // just in the collected output files will be the same as the
                    // original output files
                    Some(Vec::with_capacity(project.output_files.len()))
                } else {
                    None
                },
                task: *project,
                roots,
                cache_output_dir: None,
                digest: None,
            });
        }

        if !includes.is_empty() {
            // for some reason, ignore doesn't like it when a folder is ignored
            // and a file inside the ignored folder is included
            // so include all the parent folders of the included file
            // so that ignore walks to it and doesn't ignore it
            let mut forced_includes = HashSet::with_capacity(includes.len());

            for include in &includes {
                forced_includes.insert(include.to_path_buf());

                let clean = if has_globs(include.to_string_lossy().as_ref()) {
                    let clean = remove_globs(include);

                    forced_includes.insert(clean.to_path_buf());

                    clean
                } else {
                    include
                };

                for parent in clean.ancestors() {
                    // if we are in the workspace root, stop here
                    if starts_with_path(self.ws_root_dir, parent) {
                        break;
                    }

                    forced_includes.insert(parent.to_path_buf());
                }
            }
            let forced_includes =
                forced_includes.into_iter().collect::<Vec<_>>();

            let canonical_root = self.canonical_ws_root();
            let canonical_cache = self
                .sys
                .fs_canonicalize(self.cache_dir)
                .unwrap_or_else(|_| self.cache_dir.to_path_buf());

            let topmost =
                topmost_dirs(self.sys.clone(), &includes, self.ws_root_dir)
                    .into_iter()
                    .map(|p| p.to_path_buf())
                    // Walk roots bypass the walker's per-entry containment
                    // filter, so a root that resolves outside the workspace
                    // must be dropped here or its external contents would
                    // enter the hash.
                    .filter(|root| match self.sys.fs_canonicalize(root) {
                        Ok(target) => {
                            if target.starts_with(&canonical_cache) {
                                return false;
                            }
                            let contained = target.starts_with(&canonical_root);
                            if !contained {
                                tracing::warn!(
                                    root = ?root,
                                    target = ?target,
                                    "excluding walk root whose target escapes the workspace root"
                                );
                            }
                            contained
                        }
                        Err(_) => true,
                    })
                    .collect::<Vec<_>>();

            let topmost =
                topmost.iter().map(|p| p.as_path()).collect::<Vec<_>>();

            tracing::trace!("topmost: {topmost:?}");

            tracing::trace!(
                forced_includes = ?forced_includes,
                topmost = ?topmost,
                "before_walk"
            );

            let dirwalker =
                RealGlobDirWalker::config()
                    .standard_filters(true)
                    .include(forced_includes)
                    .root_dir(self.ws_root_dir)
                    .custom_ignore_filenames(vec![".omniignore".to_string()])
                    .follow_links(true)
                    .filter_entry(self.workspace_bound_predicate(
                        canonical_root,
                        canonical_cache,
                    ))
                    .build()
                    .build_walker()?;

            // Every walk root may have been dropped for escaping the
            // workspace, in which case there is nothing to enumerate.
            if !topmost.is_empty() {
                for res in dirwalker.walk_dir(&topmost)? {
                    let res = match res {
                        Ok(entry) => entry,
                        Err(err) if is_symlink_loop(&err) => {
                            tracing::warn!(
                                error = %err,
                                "skipping symlink loop while hashing"
                            );
                            continue;
                        }
                        Err(err) => return Err(err.into()),
                    };
                    let original_file_abs_path = res.path();

                    tracing::trace!("walked path {original_file_abs_path:?}");

                    // Prefer the cheap `d_type` reported by the walker's
                    // `readdir`/`getdents` to avoid a per-entry `stat` syscall
                    // (dispatched via `spawn_blocking`) for the common case.
                    // Symlinks and unknown types still fall back to a
                    // symlink-following stat to preserve previous behavior.
                    let is_file = match res.file_type() {
                        Some(FileType::File) => true,
                        Some(FileType::Dir | FileType::Other) => false,
                        Some(FileType::Symlink) | None => {
                            self.sys
                                .fs_is_file_async(original_file_abs_path)
                                .await?
                        }
                    };

                    if !is_file {
                        continue;
                    }

                    // Build the match candidate once per file instead of letting
                    // `GlobSet::is_match` rebuild it (path normalization, basename
                    // and extension extraction, plus allocations) for every
                    // project's input and output globset below.
                    let candidate = Candidate::new(original_file_abs_path);
                    let local_cache_policy =
                        is_local_cache_policy(original_file_abs_path);

                    for project in &mut to_process {
                        // Cheap rejection: a file can only match this project's
                        // input/output globsets if it lives under one of the
                        // literal prefixes of those globs. This skips the far more
                        // expensive `is_match_candidate` glob search (and the
                        // rooted-path construction below) for the many projects
                        // that cannot own this file.
                        if !project.match_bases.iter().any(|base| {
                            starts_with_path(original_file_abs_path, base)
                        }) {
                            continue;
                        }

                        let project_dir = project.roots[Root::Project];
                        let rooted_path = if starts_with_path(
                            original_file_abs_path,
                            project_dir,
                        ) {
                            OmniPath::new_rooted(
                                relpath(original_file_abs_path, project_dir),
                                Root::Project,
                            )
                        } else if starts_with_path(
                            original_file_abs_path,
                            self.ws_root_dir,
                        ) {
                            OmniPath::new_rooted(
                                relpath(
                                    original_file_abs_path,
                                    self.ws_root_dir,
                                ),
                                Root::Workspace,
                            )
                        } else {
                            OmniPath::new(original_file_abs_path)
                        };

                        if let Some(input_files_globset) =
                            project.input_files_globset.as_ref()
                            && (!local_cache_policy
                                || project
                                    .worktree_inputs_globset
                                    .as_ref()
                                    .is_some_and(|patterns| {
                                        patterns.is_match_candidate(&candidate)
                                    }))
                            && input_files_globset
                                .is_match_candidate(&candidate)
                            && !exclude_matches(
                                project.input_files_exclude_globset.as_ref(),
                                &candidate,
                            )
                            && let Some(resolved_input_files) =
                                project.resolved_input_files.as_mut()
                        {
                            tracing::trace!(
                                file = ?original_file_abs_path,
                                "found_input_file",
                            );
                            resolved_input_files.push(rooted_path.clone());
                        }

                        if let Some(output_files_globset) =
                            project.output_files_globset.as_ref()
                            && output_files_globset
                                .is_match_candidate(&candidate)
                            && !exclude_matches(
                                project.output_files_exclude_globset.as_ref(),
                                &candidate,
                            )
                            && let Some(resolved_output_files) =
                                project.resolved_output_files.as_mut()
                        {
                            tracing::trace!(
                                file = ?original_file_abs_path,
                                "found_output_file",
                            );
                            resolved_output_files.push(rooted_path);
                        }
                    }
                }
            }
        }

        if config.digests || config.cache_output_dirs {
            for holder in &mut to_process {
                let hash = self
                    .get_digest(&HashInput {
                        task_name: holder.task.task_name,
                        task_exec: holder.task.task_exec,
                        task_retry_exec: holder.task.task_retry_exec,
                        project_name: holder.task.project_name,
                        project_dir: holder.task.project_dir,
                        input_files: holder
                            .resolved_input_files
                            .as_ref()
                            .expect("should be some"),
                        input_env_cache_keys: holder.task.input_env_keys,
                        env_vars: holder.task.env_vars,
                        dependency_digests: holder.task.dependency_digests,
                        cached_output_files_glob: &holder.output_files_glob,
                        cached_output_files_exclude_glob: &holder
                            .output_files_exclude_glob,
                        args: holder.task.args,
                        effective_digest: holder.task.effective_digest,
                    })
                    .await?;

                holder.digest = Some(hash);
            }
        }

        if config.cache_output_dirs {
            for holder in &mut to_process {
                let hashstring = bs58::encode(
                    holder.digest.as_ref().expect("should be some"),
                )
                .into_string();
                let output_dir =
                    self.get_output_dir(holder.task.project_name, &hashstring)?;

                holder.cache_output_dir = Some(output_dir);
            }
        }

        Ok(to_process
            .into_iter()
            .map(|p| CollectResult {
                task: p.task,
                input_files: p.resolved_input_files,
                output_files: p.resolved_output_files,
                roots: p.roots,
                cache_output_dir: p.cache_output_dir,
                digest: p.digest,
            })
            .collect::<Vec<_>>())
    }
}

// Context canonicalizes workspace roots. Absolute patterns may instead use an
// alias such as /var on macOS or a non-verbatim Windows path. Align these with
// the walker's namespace for both the match-base prefilter and compiled glob.
// Only canonicalize the literal prefix; the glob itself cannot be resolved.
// Preserve paths already under the walk root, including relative symlink paths
// and their filesystem-sensitive `..` components. Direct Collector callers may
// use a noncanonical root, in which case the walker retains that namespace.
struct PatternAlignment<'a, TSys> {
    workspace_root: &'a Path,
    sys: &'a TSys,
    root_is_canonical: Option<bool>,
    // None also memoizes failed resolutions, so missing prefixes are not
    // repeatedly probed. A fresh collection retries them.
    canonical_bases: HashMap<PathBuf, Option<PathBuf>>,
}

impl<'a, TSys: FsCanonicalize> PatternAlignment<'a, TSys> {
    fn new(workspace_root: &'a Path, sys: &'a TSys) -> Self {
        Self {
            workspace_root,
            sys,
            root_is_canonical: None,
            canonical_bases: HashMap::new(),
        }
    }

    fn align(&mut self, path: PathBuf) -> PathBuf {
        // Common case: no filesystem access, cache lookup or allocation.
        if starts_with_path(&path, self.workspace_root) {
            return path;
        }

        let root_is_canonical =
            *self.root_is_canonical.get_or_insert_with(|| {
                self.sys
                    .fs_canonicalize(self.workspace_root)
                    .is_ok_and(|canonical| canonical == self.workspace_root)
            });
        if !root_is_canonical {
            return path;
        }

        let base = remove_globs(&path);
        if !self.canonical_bases.contains_key(base) {
            self.canonical_bases.insert(
                base.to_path_buf(),
                self.sys.fs_canonicalize(base).ok(),
            );
        }
        if let Some(canonical) = self.canonical_bases[base].as_ref() {
            let suffix = path.strip_prefix(base).expect("literal glob prefix");
            return if suffix.as_os_str().is_empty() {
                canonical.clone()
            } else {
                canonical.join(suffix)
            };
        }
        path
    }
}

fn populate_includes_and_patterns(
    files: &[OmniPath],
    project_dir: &Path,
    includes: &mut Vec<PathBuf>,
    match_bases: &mut Vec<PathBuf>,
    roots: &RootMap,
    patterns: &mut Vec<String>,
    alignment: &mut PatternAlignment<'_, impl FsCanonicalize>,
) -> Result<(), Error> {
    tracing::trace!(
        files = ?files,
        project_dir = ?project_dir,
        roots = ?roots,
        "populate_includes_and_patterns"
    );
    for p in files {
        let p = p.resolve(roots);

        let path = if p.is_relative() {
            std::path::absolute(project_dir.join(p))?
        } else {
            p.to_path_buf()
        };

        let path = alignment.align(path);
        patterns.push(path.to_string_lossy().into_owned());
        // The literal prefix bounds where this glob can match: any matching
        // path must live under it.
        match_bases.push(remove_globs(&path).to_path_buf());
        includes.push(path);
    }
    Ok(())
}

// Exclude patterns compile into their own globset but never seed the walk:
// they only remove already-walked files, so they do not touch `includes`,
// `match_bases`, or the walk roots. The path resolution matches
// `populate_includes_and_patterns` so a pattern compiles against the same
// walked paths.
fn resolve_exclude_patterns(
    files: &[OmniPath],
    project_dir: &Path,
    roots: &RootMap,
    patterns: &mut Vec<String>,
    alignment: &mut PatternAlignment<'_, impl FsCanonicalize>,
) -> Result<(), Error> {
    for p in files {
        let p = p.resolve(roots);

        let path = if p.is_relative() {
            std::path::absolute(project_dir.join(p))?
        } else {
            p.to_path_buf()
        };

        let path = alignment.align(path);
        patterns.push(path.to_string_lossy().into_owned());
    }
    Ok(())
}

fn exclude_matches(
    globset: Option<&Arc<GlobSet>>,
    candidate: &Candidate,
) -> bool {
    globset.is_some_and(|g| g.is_match_candidate(candidate))
}

fn is_local_cache_policy(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(omni_constants::is_worktree_configuration_name)
}

// A symlink cycle surfaces as `Error::Loop` (possibly wrapped), which is not an
// I/O error and so is not caught by the walker's not-found handling. Detecting
// it lets the collector skip the cycle instead of aborting the whole hash.
fn is_symlink_loop(err: &dir_walker::impls::IgnoreError) -> bool {
    use dir_walker::impls::IgnoreError;
    match err {
        IgnoreError::Loop { .. } => true,
        IgnoreError::WithLineNumber { err, .. }
        | IgnoreError::WithPath { err, .. }
        | IgnoreError::WithDepth { err, .. } => is_symlink_loop(err),
        IgnoreError::Partial(errs) => errs.iter().any(is_symlink_loop),
        _ => false,
    }
}

/// Operands get their own label so a shell string that happens to equal an
/// operand encoding can never share a digest with it.
fn command_hash_input(
    label: &str,
    command: &omni_command_config::CommandConfig,
) -> Option<String> {
    let canonical = command.canonical();
    if canonical.is_empty() {
        return None;
    }
    if command.is_operands() {
        Some(format!("operands_{label}={canonical}"))
    } else {
        Some(format!("{label}={canonical}"))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::atomic::{AtomicU32, Ordering},
    };

    use omni_command_config::CommandConfig;

    use super::*;

    #[test]
    fn operands_never_share_a_hash_input_with_a_shell_string() {
        use omni_command_config::{CommandConfig, CommandOperand};

        let operands =
            CommandConfig::operands([CommandOperand::literal("echo")]);
        let shell = CommandConfig::shell(operands.canonical());
        assert_eq!(operands.canonical(), shell.canonical());
        assert_ne!(
            command_hash_input("command", &operands),
            command_hash_input("command", &shell)
        );
        assert_eq!(
            command_hash_input("command", &shell),
            Some(format!("command={}", shell.canonical()))
        );
        assert_eq!(
            command_hash_input("retry_command", &operands),
            Some(format!("operands_retry_command={}", operands.canonical()))
        );
    }

    #[derive(Default)]
    struct CountingCanonicalizer {
        resolutions: HashMap<PathBuf, PathBuf>,
        calls: std::cell::RefCell<Vec<PathBuf>>,
    }

    impl system_traits::BaseFsCanonicalize for CountingCanonicalizer {
        fn base_fs_canonicalize(
            &self,
            path: &Path,
        ) -> std::io::Result<PathBuf> {
            self.calls.borrow_mut().push(path.to_path_buf());
            self.resolutions.get(path).cloned().ok_or_else(|| {
                std::io::Error::from(std::io::ErrorKind::NotFound)
            })
        }
    }

    #[test]
    fn pattern_alignment_fast_path_does_not_canonicalize() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sys = CountingCanonicalizer::default();
        let mut alignment = PatternAlignment::new(root, &sys);
        for suffix in ["proj/src/*.txt", "proj/../shared/*.txt"] {
            let path = root.join(suffix);
            assert_eq!(alignment.align(path.clone()), path);
        }
        assert!(sys.calls.borrow().is_empty());
        assert!(alignment.root_is_canonical.is_none());
        assert!(alignment.canonical_bases.is_empty());
    }

    #[test]
    fn pattern_alignment_memoizes_literal_bases_only_within_a_collection() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let alias_base = temp.path().join("alias/shared");
        let canonical_base = root.join("shared");
        let sys = CountingCanonicalizer {
            resolutions: HashMap::from([
                (root.clone(), root.clone()),
                (alias_base.clone(), canonical_base.clone()),
            ]),
            ..Default::default()
        };
        for collection in 1..=2 {
            let mut alignment = PatternAlignment::new(&root, &sys);
            for suffix in ["*.txt", "*.rs", "**/*.txt", "*.txt"] {
                assert_eq!(
                    alignment.align(alias_base.join(suffix)),
                    canonical_base.join(suffix),
                );
            }
            // One root check and one literal-prefix resolution per collection.
            assert_eq!(sys.calls.borrow().len(), collection * 2);
        }
    }

    #[test]
    fn pattern_alignment_memoizes_missing_bases_but_retries_next_collection() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let base = temp.path().join("missing");
        let mut sys = CountingCanonicalizer {
            resolutions: HashMap::from([(root.clone(), root.clone())]),
            ..Default::default()
        };
        {
            let mut alignment = PatternAlignment::new(&root, &sys);
            for suffix in ["*.txt", "*.rs", "*.txt"] {
                let path = base.join(suffix);
                assert_eq!(alignment.align(path.clone()), path);
            }
            assert_eq!(sys.calls.borrow().len(), 2);
        }
        let resolved = root.join("shared");
        sys.resolutions.insert(base.clone(), resolved.clone());
        let mut alignment = PatternAlignment::new(&root, &sys);
        assert_eq!(alignment.align(base.join("*.txt")), resolved.join("*.txt"));
        assert_eq!(sys.calls.borrow().len(), 4);
    }

    #[test]
    fn pattern_alignment_checks_noncanonical_root_only_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("alias");
        let sys = CountingCanonicalizer {
            resolutions: HashMap::from([(
                root.clone(),
                temp.path().join("workspace"),
            )]),
            ..Default::default()
        };
        let mut alignment = PatternAlignment::new(&root, &sys);
        for name in ["first/*.txt", "second/*.rs", "first/*.txt"] {
            let path = temp.path().join(name);
            assert_eq!(alignment.align(path.clone()), path);
        }
        assert_eq!(*sys.calls.borrow(), vec![root.clone()]);
        assert!(alignment.canonical_bases.is_empty());
    }

    // Ensures each collect that must observe fresh on-disk state uses its own
    // hash index, sidestepping the hasher's mtime-based cache.
    static CACHE_SEQ: AtomicU32 = AtomicU32::new(0);

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path, content).expect("write file");
    }

    fn fresh_cache_dir(root: &Path) -> PathBuf {
        let n = CACHE_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = root.join(".omni").join(format!("cache{n}"));
        std::fs::create_dir_all(&dir).expect("create cache dir");
        dir
    }

    /// Owns the backing data so we can hand out borrowed [`ProjectTaskInfo`]s.
    struct Project {
        project_name: String,
        project_dir: PathBuf,
        task_name: String,
        input_files: Vec<OmniPath>,
        input_files_exclude: Vec<OmniPath>,
        output_files: Vec<OmniPath>,
        output_files_exclude: Vec<OmniPath>,
        input_env_keys: Vec<String>,
        env_vars: Map<String, String>,
        dependency_digests: Vec<DefaultHash>,
        args: Map<String, serde_json::Value>,
        effective_digest: DefaultHash,
    }

    impl Project {
        fn new(root: &Path, name: &str) -> Self {
            Self {
                project_name: name.to_string(),
                project_dir: root.join(name),
                task_name: "build".to_string(),
                input_files: vec![],
                input_files_exclude: vec![],
                output_files: vec![],
                output_files_exclude: vec![],
                input_env_keys: vec![],
                env_vars: Map::default(),
                dependency_digests: vec![],
                args: Map::default(),
                effective_digest:
                    crate::effective_task::EffectiveTaskConfig::default()
                        .digest(),
            }
        }

        fn info(&self) -> ProjectTaskInfo<'_> {
            ProjectTaskInfo {
                project_name: &self.project_name,
                project_dir: &self.project_dir,
                task_name: &self.task_name,
                task_exec: None,
                task_retry_exec: None,
                output_files: &self.output_files,
                output_files_exclude: &self.output_files_exclude,
                input_files: &self.input_files,
                input_files_exclude: &self.input_files_exclude,
                input_env_keys: &self.input_env_keys,
                env_vars: &self.env_vars,
                dependency_digests: &self.dependency_digests,
                args: &self.args,
                effective_digest: self.effective_digest,
            }
        }
    }

    /// Owned, borrow-free view of a [`CollectResult`] so assertions can run
    /// after the collector (and its borrows) are dropped.
    struct Collected {
        input_files: Option<Vec<PathBuf>>,
        output_files: Option<Vec<PathBuf>>,
        digest: Option<DefaultHash>,
        cache_output_dir: Option<PathBuf>,
    }

    impl From<CollectResult<'_>> for Collected {
        fn from(r: CollectResult<'_>) -> Self {
            let sorted = |v: Option<Vec<OmniPath>>| {
                v.map(|files| {
                    let mut paths = files
                        .iter()
                        .map(|p| p.unresolved_path().to_path_buf())
                        .collect::<Vec<_>>();
                    paths.sort();
                    paths
                })
            };

            Collected {
                input_files: sorted(r.input_files),
                output_files: sorted(r.output_files),
                digest: r.digest,
                cache_output_dir: r.cache_output_dir,
            }
        }
    }

    async fn run_collect(
        root: &Path,
        cache_dir: &Path,
        projects: &[&Project],
        config: &CollectConfig,
    ) -> Vec<Collected> {
        let collector = Collector::new(root, cache_dir, RealSys);
        let infos = projects.iter().map(|p| p.info()).collect::<Vec<_>>();
        collector
            .collect(&infos, config)
            .await
            .expect("collect failed")
            .into_iter()
            .map(Collected::from)
            .collect()
    }

    async fn collect_one(
        root: &Path,
        cache_dir: &Path,
        project: &Project,
        config: &CollectConfig,
    ) -> Collected {
        let mut results =
            run_collect(root, cache_dir, &[project], config).await;
        assert_eq!(results.len(), 1, "expected exactly one result");
        results.pop().unwrap()
    }

    fn rel_paths(paths: &[&str]) -> BTreeSet<PathBuf> {
        paths
            .iter()
            .map(|p| p.split('/').collect::<PathBuf>())
            .collect()
    }

    fn as_set(paths: &Option<Vec<PathBuf>>) -> BTreeSet<PathBuf> {
        paths
            .as_ref()
            .expect("expected Some(files)")
            .iter()
            .cloned()
            .collect()
    }

    /// Test-only copy of `Collector::get_digest` with the identity leaf
    /// supplied by the caller, so one key can be evaluated with the legacy
    /// joined-label leaf and with the v1 leaf over identical other inputs.
    async fn reference_digest(
        collector: &Collector<'_>,
        info: &ProjectTaskInfo<'_>,
        resolved_inputs: &[OmniPath],
        identity_leaf: DefaultHash,
        trailing_leaves: &[DefaultHash],
    ) -> DefaultHash {
        let mut tree = collector
            .dir_hasher
            .hash_tree::<DefaultHasher>(
                info.project_name,
                info.project_dir,
                resolved_inputs,
            )
            .await
            .expect("hash tree");

        let mut deps = info.dependency_digests.to_vec();
        deps.sort();
        for dep in deps {
            tree.insert(dep);
        }

        if !info.env_vars.is_empty() {
            let mut keys = info
                .input_env_keys
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>();
            keys.sort();
            let lines = keys
                .into_iter()
                .map(|k| {
                    let v = info.env_vars.get(k).map_or("", |v| v.as_str());
                    format!("{k}={v}")
                })
                .collect::<Vec<_>>();
            tree.insert(DefaultHasher::hash(lines.join("\n").as_bytes()));
        }

        if !info.args.is_empty() {
            let mut entries = info.args.iter().collect::<Vec<_>>();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let lines = entries
                .into_iter()
                .map(|(k, v)| format!("{k}={v:?}"))
                .collect::<Vec<_>>();
            tree.insert(DefaultHasher::hash(lines.join("\n").as_bytes()));
        }

        let mut globs = info
            .output_files
            .iter()
            .map(|p| p.unresolved_path())
            .collect::<Vec<_>>();
        globs.sort();
        for path in globs {
            tree.insert(DefaultHasher::hash(path.to_string_lossy().as_bytes()));
        }

        if !info.output_files_exclude.is_empty() {
            let mut excludes = info
                .output_files_exclude
                .iter()
                .map(|p| p.unresolved_path())
                .collect::<Vec<_>>();
            excludes.sort();
            for path in excludes {
                tree.insert(DefaultHasher::hash(
                    format!("exclude:{}", path.to_string_lossy()).as_bytes(),
                ));
            }
        }

        if let Some(command) = info.task_exec
            && let Some(input) = command_hash_input("command", command)
        {
            tree.insert(DefaultHasher::hash(input.as_bytes()));
        }
        if let Some(command) = info.task_retry_exec
            && let Some(input) = command_hash_input("retry_command", command)
        {
            tree.insert(DefaultHasher::hash(input.as_bytes()));
        }

        tree.insert(identity_leaf);
        for leaf in trailing_leaves {
            tree.insert(*leaf);
        }
        tree.commit();
        tree.root().expect("root")
    }

    /// RFC 0024 F06.3 (decision D1): the cache key version leaf,
    /// `BLAKE3(b"omni.cache-key\0" || u32_le(version))`, inserted after the
    /// identity leaf and before the effective-task leaf. Test-only oracle,
    /// independent of the production leaf functions.
    fn cache_key_version_leaf_v1(version: u32) -> DefaultHash {
        omni_hasher::impls::hash_parts([
            b"omni.cache-key\0".as_slice(),
            version.to_le_bytes().as_slice(),
        ])
    }

    /// RFC 0024 F06.3, as a test-only oracle independent of the production
    /// leaf function: `b"omni.effective-task\0" || 0x01 || digest(32)`. It sits
    /// after the identity leaf, so every earlier leaf keeps its position.
    fn effective_leaf_v1(effective_digest: DefaultHash) -> DefaultHash {
        omni_hasher::impls::hash_parts([
            b"omni.effective-task\0\x01".as_slice(),
            effective_digest.as_slice(),
        ])
    }

    /// An ordinary undecorated task with controlled inputs, env, args, output
    /// globs, command and dependency digests.
    fn ordinary_task(root: &Path, project: &str, task: &str) -> Project {
        // A flat input keeps the hashed `@project/<path>` free of a path
        // separator, which the collector renders natively (`\` on Windows),
        // so the pinned digests hold on every OS.
        write_file(&root.join(project).join("a.txt"), "a");
        let mut p = Project::new(root, project);
        p.task_name = task.to_string();
        p.input_files = vec![OmniPath::new("*.txt")];
        p.output_files = vec![OmniPath::new("dist/**")];
        p.input_env_keys = vec!["MODE".to_string()];
        p.env_vars.insert("MODE".to_string(), "release".to_string());
        p.args
            .insert("profile".to_string(), serde_json::json!("fast"));
        p.dependency_digests = vec![[2; 32], [1; 32]];
        p
    }

    struct Digests {
        collector: DefaultHash,
        legacy_key: DefaultHash,
        /// The identity-v1 key without the F06.3 trailing leaves.
        v1_key: DefaultHash,
        /// The v1 key plus the cache key version leaf and the effective leaf
        /// of the project's effective digest, from the test-only oracle.
        final_key: DefaultHash,
    }

    async fn digests_for(
        root: &Path,
        project: &Project,
        command: &CommandConfig,
    ) -> Digests {
        let cache_dir = fresh_cache_dir(root);
        let collector = Collector::new(root, &cache_dir, RealSys);
        let info = ProjectTaskInfo {
            task_exec: Some(command),
            ..project.info()
        };
        let result = collector
            .collect(
                &[info],
                &CollectConfig {
                    digests: true,
                    ..Default::default()
                },
            )
            .await
            .expect("collect")
            .into_iter()
            .next()
            .expect("one result");
        let inputs = result.input_files.clone().expect("inputs");

        let legacy_leaf = DefaultHasher::hash(
            format!("{}#{}", info.project_name, info.task_name).as_bytes(),
        );
        let v1_leaf =
            identity_leaf_v1(info.project_name, info.task_name).expect("leaf");

        Digests {
            collector: result.digest.expect("digest"),
            legacy_key: reference_digest(
                &collector,
                &info,
                &inputs,
                legacy_leaf,
                &[],
            )
            .await,
            v1_key: reference_digest(&collector, &info, &inputs, v1_leaf, &[])
                .await,
            final_key: reference_digest(
                &collector,
                &info,
                &inputs,
                v1_leaf,
                &[
                    cache_key_version_leaf_v1(1),
                    effective_leaf_v1(info.effective_digest),
                ],
            )
            .await,
        }
    }

    fn hex32(hash: DefaultHash) -> String {
        hash.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[tokio::test]
    async fn legacy_key_still_reproduces_the_base_digest_and_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let project = ordinary_task(root, "proj", "build");
        let command = CommandConfig::shell("echo build");

        let digests = digests_for(root, &project, &command).await;

        assert_eq!(
            hex32(DefaultHasher::hash(b"proj#build")),
            "3a89953840fde7533a43a03184a9864798d00017163735990a8a93d7caf39338"
        );
        assert_eq!(
            hex32(digests.legacy_key),
            "38f3811e830a6572f5b6d9dd7e4ccf4c15c565b80f745524a34715f2f140518c"
        );
        assert_ne!(digests.collector, digests.legacy_key);
    }

    #[tokio::test]
    async fn key_with_output_excludes_matches_the_collector() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mut project = ordinary_task(root, "proj", "build");
        project.output_files_exclude = vec![OmniPath::new("dist/tmp/**")];
        let command = CommandConfig::shell("echo build");

        let with = digests_for(root, &project, &command).await;
        let plain =
            digests_for(root, &ordinary_task(root, "proj", "build"), &command)
                .await;

        assert_eq!(with.collector, with.final_key);
        assert_ne!(with.collector, plain.collector);
    }

    #[tokio::test]
    async fn v1_key_changes_only_the_identity_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let project = ordinary_task(root, "proj", "build");
        let command = CommandConfig::shell("echo build");

        let digests = digests_for(root, &project, &command).await;

        assert_eq!(
            hex32(identity_leaf_v1("proj", "build").unwrap()),
            "262db1a029777c8c34df7d061a04e010c943bb983c789b8a302619e8352d111e"
        );
        assert_eq!(
            hex32(digests.v1_key),
            "baccd2c423a6c8afbd93c453f56926ea0e49c3d837fe0909f9ecc24cd085e174"
        );
        assert_ne!(digests.v1_key, digests.legacy_key);
        // The collector's key is the v1 key plus the two trailing leaves.
        assert_ne!(digests.collector, digests.v1_key);
        assert_eq!(digests.collector, digests.final_key);
    }

    /// RFC 0024 F06.3 proof: the final key is the v1 key inputs plus the cache
    /// key version leaf (D1) and the effective-task leaf, in that order,
    /// inserted after the identity leaf. The fixture's effective digest is that
    /// of the default config.
    #[tokio::test]
    async fn the_final_key_carries_the_effective_task_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let project = ordinary_task(root, "proj", "build");
        let command = CommandConfig::shell("echo build");
        let config = crate::effective_task::EffectiveTaskConfig::default();

        let digests = digests_for(root, &project, &command).await;
        let expected = {
            let cache_dir = fresh_cache_dir(root);
            let collector = Collector::new(root, &cache_dir, RealSys);
            let info = ProjectTaskInfo {
                task_exec: Some(&command),
                ..project.info()
            };
            let inputs = collector
                .collect(
                    &[info],
                    &CollectConfig {
                        digests: true,
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
                .remove(0)
                .input_files
                .unwrap();
            reference_digest(
                &collector,
                &info,
                &inputs,
                identity_leaf_v1("proj", "build").unwrap(),
                &[
                    cache_key_version_leaf_v1(1),
                    effective_leaf_v1(config.digest()),
                ],
            )
            .await
        };

        assert_eq!(
            digests.collector, expected,
            "the collector must insert the effective leaf after the identity leaf"
        );
        assert_ne!(expected, digests.v1_key);
    }

    /// RFC 0024 F06.3 proof (FAILS on the base): golden vector of the final
    /// key for the ordinary task with a default effective config, computed from
    /// the oracle above. Pinned so the key cannot change by accident.
    #[tokio::test]
    async fn the_final_key_golden_vector_for_the_ordinary_task() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let project = ordinary_task(root, "proj", "build");
        let command = CommandConfig::shell("echo build");

        let digests = digests_for(root, &project, &command).await;

        // Re-pinned for the three unmerged env layers (new encoding bytes).
        // Before: e5e46763eebbd009eba7d973a96e6f3296a6b52bfad1e45f83f88ceef27168cd,
        // which reproduces with the layers merged into one env field.
        assert_eq!(
            hex32(digests.collector),
            "506bf3a26d95e8083a453e1483325c1fcb3b39b88611b6988002e73f2d2a0074",
        );
    }

    /// The effective digest is a mandatory key input: with no input files, no
    /// env, no args and no output globs (everything `cache.key.defaults: false`
    /// can switch off) a different effective digest is a different key.
    #[tokio::test]
    async fn the_effective_digest_alone_changes_the_key() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let config = CollectConfig {
            digests: true,
            ..Default::default()
        };
        let digest_for = |effective: DefaultHash| {
            let mut project = Project::new(root, "proj");
            project.effective_digest = effective;
            let cache_dir = fresh_cache_dir(root);
            async move {
                collect_one(root, &cache_dir, &project, &config)
                    .await
                    .digest
                    .expect("digest")
            }
        };
        let one = digest_for([1; 32]).await;
        assert_eq!(one, digest_for([1; 32]).await);
        assert_ne!(one, digest_for([2; 32]).await);
    }

    /// Bumping the cache key version is deliberate: it is a cold start for
    /// every user (`docs/patterns/cache-keys.md`). Change this pin only with
    /// the change that bumps the constant.
    #[test]
    fn the_cache_key_version_is_pinned() {
        assert_eq!(CACHE_KEY_VERSION, 1);
    }

    /// Two pairs with equal non-identity inputs shared a digest on the base
    /// because both rendered `a#b#c`; the v1 leaf splits them.
    #[tokio::test]
    async fn formerly_colliding_pairs_have_distinct_digests() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let command = CommandConfig::shell("echo same");
        let first = ordinary_task(root, "a#b", "c");
        let second = ordinary_task(root, "a", "b#c");

        let first_digests = digests_for(root, &first, &command).await;
        let second_digests = digests_for(root, &second, &command).await;

        assert_ne!(first_digests.collector, second_digests.collector);
    }

    /// Documents the base collision through the test-only oracle and shows the v1 leaf separates the pair.
    #[tokio::test]
    async fn base_key_collides_and_v1_key_separates_the_pair() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let command = CommandConfig::shell("echo same");
        let first = ordinary_task(root, "a#b", "c");
        let second = ordinary_task(root, "a", "b#c");

        let first_digests = digests_for(root, &first, &command).await;
        let second_digests = digests_for(root, &second, &command).await;

        assert_eq!(first_digests.legacy_key, second_digests.legacy_key);
        assert_ne!(first_digests.v1_key, second_digests.v1_key);
    }

    #[tokio::test]
    async fn args_digest_ignores_insertion_order() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let config = CollectConfig {
            digests: true,
            ..Default::default()
        };
        let digest_for = |keys: &[&str]| {
            let mut project = Project::new(root, "proj");
            for k in keys {
                project.args.insert(k.to_string(), serde_json::json!(*k));
            }
            let cache_dir = fresh_cache_dir(root);
            async move {
                collect_one(root, &cache_dir, &project, &config)
                    .await
                    .digest
                    .expect("digest")
            }
        };
        let sorted = digest_for(&["a", "b", "z"]).await;
        assert_eq!(sorted, digest_for(&["z", "a", "b"]).await);
        assert_eq!(sorted, digest_for(&["b", "z", "a"]).await);
        // Golden, re-pinned to the cache key version and effective-task leaves
        // (RFC 0024 F06.3). Before F06.3: [226, 33, 98, 53, ..., 47, 76, 56].
        assert_eq!(
            format!("{sorted:?}"),
            "[11, 70, 242, 132, 104, 105, 225, 181, 135, 78, 116, 85, 56, 12, 201, 180, 198, 252, 117, 29, 241, 218, 135, 229, 56, 59, 106, 59, 168, 231, 65, 81]"
        );
    }

    #[tokio::test]
    async fn an_output_base_moves_entries_but_not_the_input_walk_exclusion() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write_file(&root.join("proj/source.txt"), "source");
        // The cache directory sits inside the project the inputs are globbed in.
        let cache_dir = root.join("proj/.cache");
        let ours = cache_dir.join(".partitions/trusted/e1");
        std::fs::create_dir_all(&ours).unwrap();
        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("**/*")];
        let config = CollectConfig {
            input_files: true,
            digests: true,
            cache_output_dirs: true,
            ..Default::default()
        };
        let collect = || async {
            let collector = Collector::new(root, &cache_dir, RealSys)
                .with_output_base(&ours);
            let mut out =
                collector.collect(&[project.info()], &config).await.unwrap();
            Collected::from(out.pop().unwrap())
        };
        let before = collect().await;
        assert!(
            before.cache_output_dir.as_ref().unwrap().starts_with(&ours),
            "{:?}",
            before.cache_output_dir
        );
        // Another partition's content is not an input, so it cannot move a key.
        write_file(
            &cache_dir.join(".partitions/untrusted/e1/p/s/planted.txt"),
            "x",
        );
        let after = collect().await;
        assert_eq!(before.input_files, after.input_files);
        assert_eq!(before.digest, after.digest);
        assert_eq!(as_set(&before.input_files), rel_paths(&["source.txt"]));
    }

    #[tokio::test]
    async fn local_worktree_policy_is_not_a_glob_input_but_can_be_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write_file(&root.join("proj/source.txt"), "source");
        write_file(
            &root.join("proj/worktree.omni.yaml"),
            "cache: {dir: first}",
        );
        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("**/*")];
        let config = CollectConfig {
            input_files: true,
            digests: true,
            ..Default::default()
        };
        let before =
            collect_one(root, &fresh_cache_dir(root), &project, &config).await;
        assert_eq!(as_set(&before.input_files), rel_paths(&["source.txt"]));
        write_file(
            &root.join("proj/worktree.omni.yaml"),
            "cache: {dir: changed}",
        );
        let after =
            collect_one(root, &fresh_cache_dir(root), &project, &config).await;
        assert_eq!(before.digest, after.digest);
        project.input_files = vec![OmniPath::new("worktree.omni.yaml")];
        let explicit =
            collect_one(root, &fresh_cache_dir(root), &project, &config).await;
        assert_eq!(
            as_set(&explicit.input_files),
            rel_paths(&["worktree.omni.yaml"])
        );
        project.input_files = vec![OmniPath::new("worktree.omni.*")];
        let targeted =
            collect_one(root, &fresh_cache_dir(root), &project, &config).await;
        assert_eq!(
            as_set(&targeted.input_files),
            rel_paths(&["worktree.omni.yaml"])
        );
    }

    #[tokio::test]
    async fn custom_cache_is_never_collected_as_input_or_output() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let cache = root.join("proj/custom-cache");
        write_file(&root.join("proj/source.txt"), "source");
        write_file(&cache.join("cached.txt"), "cache state");
        let mut project = Project::new(&root, "proj");
        let config = CollectConfig {
            input_files: true,
            output_files: true,
            digests: true,
            cache_output_dirs: false,
        };
        for pattern in ["**/*", "custom-cache/**/*", "custom-cache/cached.txt"]
        {
            project.input_files = vec![OmniPath::new(pattern)];
            project.output_files = project.input_files.clone();
            let before = collect_one(&root, &cache, &project, &config).await;
            let expected = if pattern == "**/*" {
                rel_paths(&["source.txt"])
            } else {
                BTreeSet::new()
            };
            assert_eq!(as_set(&before.input_files), expected);
            assert_eq!(as_set(&before.output_files), expected);
            write_file(
                &cache.join("cached.txt"),
                "changed cache state with a different size",
            );
            let after = collect_one(&root, &cache, &project, &config).await;
            assert_eq!(before.digest, after.digest);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn explicit_worktree_policy_input_works_with_an_aliased_root() {
        let temp = tempfile::tempdir().unwrap();
        let actual = temp.path().join("actual");
        write_file(&actual.join("proj/worktree.omni.yaml"), "{}");
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let mut project = Project::new(&alias, "proj");
        project.input_files = vec![OmniPath::new("worktree.omni.yaml")];
        let result = collect_one(
            &alias,
            &fresh_cache_dir(&alias),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["worktree.omni.yaml"])
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_into_custom_cache_are_not_collected() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let cache = root.join("state");
        write_file(&cache.join("cached.txt"), "cache state");
        std::fs::create_dir_all(root.join("proj")).unwrap();
        std::os::unix::fs::symlink(&cache, root.join("proj/link")).unwrap();
        let mut project = Project::new(&root, "proj");
        for pattern in ["**/*", "link/**/*", "link/cached.txt"] {
            project.input_files = vec![OmniPath::new(pattern)];
            project.output_files = project.input_files.clone();
            let result = collect_one(
                &root,
                &cache,
                &project,
                &CollectConfig {
                    input_files: true,
                    output_files: true,
                    digests: true,
                    cache_output_dirs: false,
                },
            )
            .await;
            assert!(as_set(&result.input_files).is_empty());
            assert!(as_set(&result.output_files).is_empty());
        }
    }

    #[tokio::test]
    async fn resolves_matching_input_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/a.txt"), "a");
        write_file(&root.join("proj/src/nested/b.txt"), "b");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["src/a.txt", "src/nested/b.txt"]),
        );
        // Output collection was not requested.
        assert!(result.output_files.is_none());
    }

    #[tokio::test]
    async fn resolves_matching_output_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/dist/a.js"), "a");
        write_file(&root.join("proj/dist/b.js"), "b");
        write_file(&root.join("proj/dist/skip.map"), "m");

        let mut project = Project::new(root, "proj");
        project.output_files = vec![OmniPath::new("dist/**/*.js")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                output_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.output_files),
            rel_paths(&["dist/a.js", "dist/b.js"]),
        );
    }

    #[tokio::test]
    async fn excludes_files_not_matching_glob() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/keep.txt"), "keep");
        write_file(&root.join("proj/src/ignore.md"), "ignore");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(as_set(&result.input_files), rel_paths(&["src/keep.txt"]));
    }

    #[tokio::test]
    async fn exclude_removes_a_subtree_the_include_walks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/a.txt"), "a");
        write_file(&root.join("proj/src/gen/b.txt"), "b");
        write_file(&root.join("proj/src/gen/nested/c.txt"), "c");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];
        project.input_files_exclude = vec![OmniPath::new("src/gen/**")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        // Only the include-and-not-exclude file survives; the excluded subtree
        // is dropped even though the include glob matched it.
        assert_eq!(as_set(&result.input_files), rel_paths(&["src/a.txt"]));
    }

    #[tokio::test]
    async fn exclude_only_seeds_no_walk_and_collects_nothing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/a.txt"), "a");

        let mut project = Project::new(root, "proj");
        // An exclude with no include seeds no walk root, so nothing is
        // collected: exclude patterns never pull files in.
        project.input_files_exclude = vec![OmniPath::new("src/**")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert!(as_set(&result.input_files).is_empty());
    }

    // A glob such as `src/*` matches the directory entry `src/sub` as well as
    // `src/keep.txt`. Only regular files must be collected, exercising the
    // walker's cheap `d_type` file-type check.
    #[tokio::test]
    async fn excludes_directories_matching_glob() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/keep.txt"), "keep");
        write_file(&root.join("proj/src/sub/inner.txt"), "inner");

        let mut project = Project::new(root, "proj");
        // `*` does not cross `/`, so this matches `src/keep.txt` and the
        // directory `src/sub`, but not `src/sub/inner.txt`.
        project.input_files = vec![OmniPath::new("src/*")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["src/keep.txt"]),
            "directory entries must not be collected",
        );
    }

    #[tokio::test]
    async fn collects_nothing_with_empty_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/a.txt"), "a");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig::default(),
        )
        .await;

        assert!(result.input_files.is_none());
        assert!(result.output_files.is_none());
        assert!(result.digest.is_none());
        assert!(result.cache_output_dir.is_none());
    }

    #[tokio::test]
    async fn computes_digest_when_requested() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/a.txt"), "a");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                digests: true,
                ..Default::default()
            },
        )
        .await;

        assert!(result.digest.is_some());
        // Requesting digests implies collecting input files.
        assert!(result.input_files.is_some());
    }

    #[tokio::test]
    async fn digest_is_stable_for_identical_inputs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/a.txt"), "a");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];

        let cfg = CollectConfig {
            digests: true,
            ..Default::default()
        };
        let cache_dir = fresh_cache_dir(root);

        let first = collect_one(root, &cache_dir, &project, &cfg).await;
        let second = collect_one(root, &cache_dir, &project, &cfg).await;

        assert_eq!(first.digest, second.digest);
    }

    #[tokio::test]
    async fn digest_changes_when_file_content_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let file = root.join("proj/src/a.txt");

        write_file(&file, "original");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];

        let cfg = CollectConfig {
            digests: true,
            ..Default::default()
        };

        // Fresh index dirs avoid the hasher's mtime-based cache so the digest
        // reflects the actual file content on each run.
        let before =
            collect_one(root, &fresh_cache_dir(root), &project, &cfg).await;

        write_file(&file, "a completely different, longer body");

        let after =
            collect_one(root, &fresh_cache_dir(root), &project, &cfg).await;

        assert_ne!(before.digest, after.digest);
    }

    #[tokio::test]
    async fn digest_changes_with_env_var_values() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/src/a.txt"), "a");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];
        project.input_env_keys = vec!["KEY".to_string()];
        project
            .env_vars
            .insert("KEY".to_string(), "one".to_string());

        let cfg = CollectConfig {
            digests: true,
            ..Default::default()
        };
        let cache_dir = fresh_cache_dir(root);

        let first = collect_one(root, &cache_dir, &project, &cfg).await.digest;

        project
            .env_vars
            .insert("KEY".to_string(), "two".to_string());
        let second = collect_one(root, &cache_dir, &project, &cfg).await.digest;

        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn sets_cache_output_dir_from_digest() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let cache_dir = fresh_cache_dir(root);

        write_file(&root.join("proj/src/a.txt"), "a");

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("src/**/*.txt")];

        let result = collect_one(
            root,
            &cache_dir,
            &project,
            &CollectConfig {
                cache_output_dirs: true,
                ..Default::default()
            },
        )
        .await;

        let output_dir = result
            .cache_output_dir
            .expect("cache output dir should be set");
        // Layout: <cache_dir>/<path_safe(project)>/output/<bs58(digest)>
        assert!(output_dir.starts_with(&cache_dir));
        assert!(
            output_dir
                .components()
                .any(|c| c.as_os_str() == path_safe("proj").as_str()),
            "expected the path-safe project name as a component",
        );
        assert!(output_dir.components().any(|c| c.as_os_str() == "output"));
        // The leaf directory is the bs58-encoded digest.
        assert!(output_dir.file_name().is_some());
    }

    #[tokio::test]
    async fn buckets_files_to_owning_project() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("project1/src/one.txt"), "one");
        write_file(&root.join("project2/src/two.txt"), "two");

        let mut p1 = Project::new(root, "project1");
        p1.input_files = vec![OmniPath::new("src/**/*.txt")];
        let mut p2 = Project::new(root, "project2");
        p2.input_files = vec![OmniPath::new("src/**/*.txt")];

        let results = run_collect(
            root,
            &fresh_cache_dir(root),
            &[&p1, &p2],
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(results.len(), 2);
        // Each project only sees files under its own directory even though the
        // glob patterns are identical.
        assert_eq!(
            as_set(&results[0].input_files),
            rel_paths(&["src/one.txt"])
        );
        assert_eq!(
            as_set(&results[1].input_files),
            rel_paths(&["src/two.txt"])
        );
    }

    async fn assert_collected_scope(
        root: &Path,
        project: &Project,
        expected_inputs: &[&Path],
        expected_outputs: &[&Path],
    ) -> DefaultHash {
        let cache_dir = fresh_cache_dir(root);
        let collector = Collector::new(root, &cache_dir, RealSys);
        let infos = [project.info()];
        let results = collector
            .collect(
                &infos,
                &CollectConfig {
                    input_files: true,
                    output_files: true,
                    digests: true,
                    ..Default::default()
                },
            )
            .await
            .expect("collect sibling paths");
        let result = &results[0];
        for (files, expected) in [
            (&result.input_files, expected_inputs),
            (&result.output_files, expected_outputs),
        ] {
            let files = files.as_ref().expect("files requested");
            let mut actual = files
                .iter()
                .map(|file| {
                    std::fs::canonicalize(file.resolve(&result.roots)).unwrap()
                })
                .collect::<Vec<_>>();
            let mut expected = expected
                .iter()
                .map(|file| std::fs::canonicalize(file).unwrap())
                .collect::<Vec<_>>();
            actual.sort();
            expected.sort();
            assert_eq!(actual, expected, "patterns: {:?}", project.input_files,);
        }
        result.digest.expect("digest requested")
    }

    #[tokio::test]
    async fn collects_sibling_paths_with_and_without_local_inputs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let local = root.join("proj/local.txt");
        write_file(&local, "local");
        let shared = root.join("shared/data.txt");
        let patterns = [
            "../shared/data.txt".parse().unwrap(),
            "../shared/*.txt".parse().unwrap(),
            "@project/../shared/*.txt".parse().unwrap(),
            "@workspace/shared/*.txt".parse().unwrap(),
            OmniPath::new(shared.clone()),
            OmniPath::new(root.join("shared/*.txt")),
        ];
        // Production Context roots are canonical (verbatim on Windows), while
        // absolute paths supplied in configuration usually are not. Also retain
        // coverage for callers that pass ordinary roots directly to Collector.
        for root in [root.to_path_buf(), std::fs::canonicalize(root).unwrap()] {
            let root = root.as_path();
            for pattern in &patterns {
                for include_local in [false, true] {
                    write_file(&shared, "original");
                    let mut project = Project::new(root, "proj");
                    project.input_files = vec![pattern.clone()];
                    project.output_files = vec![pattern.clone()];
                    let mut expected_inputs = vec![shared.as_path()];
                    if include_local {
                        project.input_files.push(OmniPath::new("local.txt"));
                        expected_inputs.push(local.as_path());
                    }
                    let before = assert_collected_scope(
                        root,
                        &project,
                        &expected_inputs,
                        &[&shared],
                    )
                    .await;
                    write_file(
                        &shared,
                        "changed sibling content, with a different size",
                    );
                    let after = assert_collected_scope(
                        root,
                        &project,
                        &expected_inputs,
                        &[&shared],
                    )
                    .await;
                    assert_ne!(
                        before, after,
                        "sibling input must affect the digest"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn absolute_excludes_match_with_canonical_roots() {
        let temp = tempfile::tempdir().expect("tempdir");
        let shared = temp.path().join("shared");
        write_file(&shared.join("keep.txt"), "keep");
        write_file(&shared.join("skip.txt"), "skip");
        std::fs::create_dir_all(temp.path().join("proj")).unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();

        for exclude in [shared.join("skip.txt"), shared.join("sk*.txt")] {
            let mut project = Project::new(&root, "proj");
            project.input_files = vec![OmniPath::new(shared.join("*.txt"))];
            project.output_files = project.input_files.clone();
            project.input_files_exclude = vec![OmniPath::new(exclude)];
            project.output_files_exclude = project.input_files_exclude.clone();
            assert_collected_scope(
                &root,
                &project,
                &[&shared.join("keep.txt")],
                &[&shared.join("keep.txt")],
            )
            .await;
        }
    }

    // Reproduce macOS's /var -> /private/var alias even on Linux.
    #[cfg(unix)]
    #[tokio::test]
    async fn absolute_patterns_through_workspace_alias_match_canonical_walk() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let shared = workspace.join("shared");
        write_file(&shared.join("keep.txt"), "original");
        write_file(&shared.join("skip.txt"), "excluded");
        std::fs::create_dir_all(workspace.join("proj")).unwrap();
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&workspace, &alias).unwrap();
        let root = std::fs::canonicalize(&workspace).unwrap();

        for include in
            [alias.join("shared/keep.txt"), alias.join("shared/*.txt")]
        {
            for exclude in
                [alias.join("shared/skip.txt"), alias.join("shared/sk*.txt")]
            {
                let mut project = Project::new(&root, "proj");
                project.input_files = vec![OmniPath::new(&include)];
                project.output_files = project.input_files.clone();
                project.input_files_exclude = vec![OmniPath::new(exclude)];
                project.output_files_exclude =
                    project.input_files_exclude.clone();
                write_file(&shared.join("keep.txt"), "original");
                let before = assert_collected_scope(
                    &root,
                    &project,
                    &[&shared.join("keep.txt")],
                    &[&shared.join("keep.txt")],
                )
                .await;
                write_file(
                    &shared.join("keep.txt"),
                    "changed content with a different size",
                );
                let after = assert_collected_scope(
                    &root,
                    &project,
                    &[&shared.join("keep.txt")],
                    &[&shared.join("keep.txt")],
                )
                .await;
                assert_ne!(
                    before, after,
                    "aliased input must affect the digest"
                );
            }
        }
    }

    #[tokio::test]
    async fn parent_relative_and_absolute_paths_outside_workspace_are_excluded()
    {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        write_file(&root.join("proj/local.txt"), "local");
        let external = temp.path().join("external/data.txt");
        let patterns = [
            "../../external/data.txt".parse().unwrap(),
            "../../external/*.txt".parse().unwrap(),
            "@project/../../external/*.txt".parse().unwrap(),
            "@workspace/../external/*.txt".parse().unwrap(),
            OmniPath::new(external.clone()),
            OmniPath::new(temp.path().join("external/*.txt")),
        ];
        for pattern in patterns {
            write_file(&external, "external");
            let mut project = Project::new(&root, "proj");
            project.input_files = vec![pattern.clone()];
            project.output_files = vec![pattern];
            let config = CollectConfig {
                input_files: true,
                output_files: true,
                digests: true,
                ..Default::default()
            };
            let before =
                collect_one(&root, &fresh_cache_dir(&root), &project, &config)
                    .await;
            assert!(as_set(&before.input_files).is_empty());
            assert!(as_set(&before.output_files).is_empty());
            write_file(&external, "external edit must not enter the digest");
            let after =
                collect_one(&root, &fresh_cache_dir(&root), &project, &config)
                    .await;
            assert!(as_set(&after.input_files).is_empty());
            assert!(as_set(&after.output_files).is_empty());
            assert_eq!(before.digest, after.digest);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_to_sibling_directory_collects_inputs_and_outputs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let shared = root.join("shared/data.txt");
        write_file(&shared, "original");
        symlink(&root.join("shared"), &root.join("proj/linked"));
        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("linked/*.txt")];
        project.output_files = project.input_files.clone();
        let before =
            assert_collected_scope(root, &project, &[&shared], &[&shared])
                .await;
        write_file(&shared, "edited sibling content behind a symlink");
        let after =
            assert_collected_scope(root, &project, &[&shared], &[&shared])
                .await;
        assert_ne!(before, after);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn parent_components_after_symlink_keep_filesystem_meaning() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let local = root.join("proj/local.txt");
        let shared = root.join("target/shared/data.txt");
        write_file(&local, "local");
        write_file(&shared, "real target");
        write_file(
            &root.join("proj/shared/wrong.txt"),
            "lexical cleaning trap",
        );
        std::fs::create_dir_all(root.join("target/child")).unwrap();
        symlink(&root.join("target/child"), &root.join("proj/linked"));
        let mut project = Project::new(root, "proj");
        project.input_files = vec![
            OmniPath::new("local.txt"),
            OmniPath::new("linked/../shared/*.txt"),
        ];
        project.output_files = vec![OmniPath::new("linked/../shared/*.txt")];
        assert_collected_scope(root, &project, &[&local, &shared], &[&shared])
            .await;
    }

    #[cfg(unix)]
    fn symlink(target: &Path, link: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).expect("create link parent");
        }
        std::os::unix::fs::symlink(target, link).expect("create symlink");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_directory_contributes_to_input_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/real/a.txt"), "a");
        write_file(&root.join("proj/real/nested/b.txt"), "b");
        symlink(&root.join("proj/real"), &root.join("proj/linked"));

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("linked/**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["linked/a.txt", "linked/nested/b.txt"]),
            "content behind an in-workspace symlinked directory must be hashed",
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn editing_file_behind_symlink_changes_digest() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let target = root.join("proj/real/a.txt");

        write_file(&target, "original");
        symlink(&root.join("proj/real"), &root.join("proj/linked"));

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("linked/**/*.txt")];

        let cfg = CollectConfig {
            digests: true,
            ..Default::default()
        };

        let before =
            collect_one(root, &fresh_cache_dir(root), &project, &cfg).await;
        write_file(&target, "a completely different, longer body");
        let after =
            collect_one(root, &fresh_cache_dir(root), &project, &cfg).await;

        assert_ne!(
            before.digest, after.digest,
            "editing a file behind a symlinked directory must change the digest",
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn adding_file_behind_symlink_changes_digest() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/real/a.txt"), "a");
        symlink(&root.join("proj/real"), &root.join("proj/linked"));

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("linked/**/*.txt")];

        let cfg = CollectConfig {
            digests: true,
            ..Default::default()
        };

        let before =
            collect_one(root, &fresh_cache_dir(root), &project, &cfg).await;
        write_file(&root.join("proj/real/c.txt"), "c");
        let after =
            collect_one(root, &fresh_cache_dir(root), &project, &cfg).await;

        assert_ne!(
            before.digest, after.digest,
            "adding a file behind a symlinked directory must change the digest",
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn escaping_symlinked_directory_child_is_excluded() {
        let outside = tempfile::tempdir().expect("outside tempdir");
        write_file(&outside.path().join("secret.txt"), "secret");

        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        write_file(&root.join("proj/keep.txt"), "keep");
        symlink(outside.path(), &root.join("proj/linked"));

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["keep.txt"]),
            "content behind an escaping symlinked directory must be excluded",
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn escaping_symlinked_directory_root_is_excluded() {
        let outside = tempfile::tempdir().expect("outside tempdir");
        write_file(&outside.path().join("secret.txt"), "secret");

        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        symlink(outside.path(), &root.join("proj/linked"));

        let mut project = Project::new(root, "proj");
        // The glob points directly at the symlink, so the walk is rooted at
        // it; an escaping root must be dropped before walking.
        project.input_files = vec![OmniPath::new("linked/**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&[]),
            "an escaping symlinked directory used as a walk root must be excluded",
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn escaping_symlinked_file_excluded_but_in_workspace_link_hashed() {
        let outside = tempfile::tempdir().expect("outside tempdir");
        write_file(&outside.path().join("ext.txt"), "external");

        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        write_file(&root.join("proj/target.txt"), "target");
        symlink(
            &outside.path().join("ext.txt"),
            &root.join("proj/ext_link.txt"),
        );
        symlink(
            &root.join("proj/target.txt"),
            &root.join("proj/local_link.txt"),
        );

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["target.txt", "local_link.txt"]),
            "escaping symlinked files are excluded; in-workspace ones are kept",
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_loop_is_skipped_not_fatal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/keep.txt"), "keep");
        // A link back to an ancestor forms a cycle when followed.
        symlink(&root.join("proj"), &root.join("proj/loopself"));

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["keep.txt"]),
            "a symlink cycle must be skipped without aborting the collect",
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dangling_symlink_is_skipped() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("proj/keep.txt"), "keep");
        symlink(
            &root.join("proj/nonexistent"),
            &root.join("proj/dangling.txt"),
        );

        let mut project = Project::new(root, "proj");
        project.input_files = vec![OmniPath::new("**/*.txt")];

        let result = collect_one(
            root,
            &fresh_cache_dir(root),
            &project,
            &CollectConfig {
                input_files: true,
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            as_set(&result.input_files),
            rel_paths(&["keep.txt"]),
            "a dangling symlink must be skipped without aborting the collect",
        );
    }
}
