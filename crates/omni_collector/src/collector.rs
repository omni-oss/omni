use std::{
    collections::HashSet,
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
use omni_hasher::{
    Hasher,
    impls::{DefaultHash, DefaultHasher},
    project_dir_hasher::{ProjectDirHasher, impls::RealDirHasher},
};
use omni_types::{OmniPath, Root, RootMap};
use omni_utils::glob::build_glob_set;
use omni_utils::path::{
    has_globs, path_safe, relpath, remove_globs, starts_with_path, topmost_dirs,
};
use system_traits::{FsMetadata, FsMetadataAsync, auto_impl, impls::RealSys};
use trace::Level;

use crate::error::{Error, ErrorInner};

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
    pub input_files: &'a [OmniPath],
    pub input_env_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
    pub args: &'a Map<String, serde_json::Value>,
}

#[auto_impl]
pub trait CollectorSys:
    FsMetadata + FsMetadataAsync + Clone + Send + Sync
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
    pub input_env_cache_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
    pub args: &'a Map<String, serde_json::Value>,
}

struct Holder<'a> {
    output_files_globset: Option<Arc<GlobSet>>,
    output_files_glob: Vec<OmniPath>,
    resolved_output_files: Option<Vec<OmniPath>>,
    input_files_globset: Option<Arc<GlobSet>>,
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
    cache_dir: &'a Path,
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
        }
    }
}

impl<'a, TSys: CollectorSys> Collector<'a, TSys> {
    fn get_project_dir(&self, project_name: &str) -> PathBuf {
        let name = path_safe(project_name);

        self.cache_dir.join(name).join("output")
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
            let mut buff = vec![];
            for (key, value) in hash_input.args.iter() {
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

        let full_task_name =
            format!("{}#{}", hash_input.project_name, hash_input.task_name);

        if let Some(command) = hash_input.task_exec {
            let command = command.canonical();
            if !command.is_empty() {
                let command_str = format!("command={command}");
                tree.insert(DefaultHasher::hash(command_str.as_bytes()));
            }
        }
        if let Some(retry_command) = hash_input.task_retry_exec {
            let retry_command = retry_command.canonical();
            if !retry_command.is_empty() {
                let retry_command_str =
                    format!("retry_command={retry_command}");
                tree.insert(DefaultHasher::hash(retry_command_str.as_bytes()));
            }
        }

        tree.insert(DefaultHasher::hash(full_task_name.as_bytes()));

        tree.commit();

        Ok(tree.root().expect("unable to get root"))
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(
            level = Level::DEBUG,
            skip_all,
            fields(project_tasks_count = project_tasks.len()),
        )
    )]
    pub async fn collect(
        &self,
        project_tasks: &[ProjectTaskInfo<'a>],
        config: &CollectConfig,
    ) -> Result<Vec<CollectResult<'a>>, Error> {
        let mut to_process = Vec::with_capacity(project_tasks.len());

        trace::trace!(?project_tasks, ?config, "begin_collect");

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

        for project in project_tasks {
            trace::trace!(
                project_name = ?project.project_name,
                "processing_project"
            );
            let roots = enum_map! {
                Root::Workspace => &self.ws_root_dir,
                Root::Project => project.project_dir,
            };

            let mut match_bases = Vec::new();

            let mut output_files_globset = None;
            if should_collect_output_files {
                let mut output_patterns = Vec::new();

                populate_includes_and_patterns(
                    project.output_files,
                    project.project_dir,
                    &mut includes,
                    &mut match_bases,
                    &roots,
                    &mut output_patterns,
                )?;

                output_files_globset = Some(build_glob_set(&output_patterns)?);
            }

            let mut input_files_globset = None;

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
                )?;

                input_files_globset = Some(build_glob_set(&input_patterns)?);
            }

            to_process.push(Holder {
                input_files_globset,
                output_files_globset,
                output_files_glob: project.output_files.to_vec(),
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

            let topmost =
                topmost_dirs(self.sys.clone(), &includes, self.ws_root_dir)
                    .into_iter()
                    .map(|p| p.to_path_buf())
                    .collect::<Vec<_>>();

            let topmost =
                topmost.iter().map(|p| p.as_path()).collect::<Vec<_>>();

            log::trace!("topmost: {topmost:?}");

            trace::trace!(
                forced_includes = ?forced_includes,
                topmost = ?topmost,
                "before_walk"
            );

            let dirwalker = RealGlobDirWalker::config()
                .standard_filters(true)
                .include(forced_includes)
                .root_dir(self.ws_root_dir)
                .custom_ignore_filenames(vec![".omniignore".to_string()])
                .build()
                .build_walker()?;

            for res in dirwalker.walk_dir(&topmost)? {
                let res = res?;
                let original_file_abs_path = res.path();

                log::trace!("walked path {original_file_abs_path:?}");

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
                            relpath(original_file_abs_path, self.ws_root_dir),
                            Root::Workspace,
                        )
                    } else {
                        OmniPath::new(original_file_abs_path)
                    };

                    if let Some(input_files_globset) =
                        project.input_files_globset.as_ref()
                        && input_files_globset.is_match_candidate(&candidate)
                        && let Some(resolved_input_files) =
                            project.resolved_input_files.as_mut()
                    {
                        trace::trace!(
                            file = ?original_file_abs_path,
                            "found_input_file",
                        );
                        resolved_input_files.push(rooted_path.clone());
                    }

                    if let Some(output_files_globset) =
                        project.output_files_globset.as_ref()
                        && output_files_globset.is_match_candidate(&candidate)
                        && let Some(resolved_output_files) =
                            project.resolved_output_files.as_mut()
                    {
                        trace::trace!(
                            file = ?original_file_abs_path,
                            "found_output_file",
                        );
                        resolved_output_files.push(rooted_path);
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
                        args: holder.task.args,
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

fn populate_includes_and_patterns(
    files: &[OmniPath],
    project_dir: &Path,
    includes: &mut Vec<PathBuf>,
    match_bases: &mut Vec<PathBuf>,
    roots: &RootMap,
    patterns: &mut Vec<String>,
) -> Result<(), Error> {
    trace::trace!(
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

        patterns.push(path.to_string_lossy().into_owned());
        // The literal prefix bounds where this glob can match: any matching
        // path must live under it.
        match_bases.push(remove_globs(&path).to_path_buf());
        includes.push(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::atomic::{AtomicU32, Ordering},
    };

    use super::*;

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
        output_files: Vec<OmniPath>,
        input_env_keys: Vec<String>,
        env_vars: Map<String, String>,
        dependency_digests: Vec<DefaultHash>,
        args: Map<String, serde_json::Value>,
    }

    impl Project {
        fn new(root: &Path, name: &str) -> Self {
            Self {
                project_name: name.to_string(),
                project_dir: root.join(name),
                task_name: "build".to_string(),
                input_files: vec![],
                output_files: vec![],
                input_env_keys: vec![],
                env_vars: Map::default(),
                dependency_digests: vec![],
                args: Map::default(),
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
                input_files: &self.input_files,
                input_env_keys: &self.input_env_keys,
                env_vars: &self.env_vars,
                dependency_digests: &self.dependency_digests,
                args: &self.args,
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
}
