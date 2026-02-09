use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use dir_walker::{DirEntry as _, DirWalker as _, impls::RealGlobDirWalker};
use enum_map::enum_map;
use globset::{Glob, GlobSet, GlobSetBuilder};
use maps::Map;
use omni_hasher::{
    Hasher,
    impls::{DefaultHash, DefaultHasher},
    project_dir_hasher::{ProjectDirHasher, impls::RealDirHasher},
};
use omni_types::{OmniPath, Root, RootMap};
use omni_utils::path::{
    has_globs, path_safe, relpath, remove_globs, topmost_dirs,
};
use system_traits::{FsMetadata, FsMetadataAsync, auto_impl, impls::RealSys};

use crate::error::{Error, ErrorInner};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectConfig {
    pub output_files: bool,
    pub input_files: bool,
    pub digests: bool,
    pub cache_output_dirs: bool,
}

#[allow(clippy::too_many_arguments)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProjectTaskInfo<'a> {
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub task_name: &'a str,
    pub task_command: &'a str,
    pub output_files: &'a [OmniPath],
    pub input_files: &'a [OmniPath],
    pub input_env_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
}

#[auto_impl]
pub trait CollectorSys:
    FsMetadata + FsMetadataAsync + Clone + Send + Sync
{
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub task_command: &'a str,
    pub project_name: &'a str,
    pub project_dir: &'a Path,
    pub input_files: &'a [OmniPath],
    pub cached_output_files_glob: &'a [OmniPath],
    pub input_env_cache_keys: &'a [String],
    pub env_vars: &'a Map<String, String>,
    pub dependency_digests: &'a [DefaultHash],
}

struct Holder<'a> {
    output_files_globset: Option<GlobSet>,
    output_files_glob: Vec<OmniPath>,
    resolved_output_files: Option<Vec<OmniPath>>,
    input_files_globset: Option<GlobSet>,
    resolved_input_files: Option<Vec<OmniPath>>,
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

        let full_task_name = format!(
            "{}#{}: {}",
            hash_input.project_name,
            hash_input.task_name,
            hash_input.task_command
        );
        tree.insert(DefaultHasher::hash(full_task_name.as_bytes()));

        tree.commit();

        Ok(tree.root().expect("unable to get root"))
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "debug", skip(self, config, project_tasks),
            fields(project_tasks_count = project_tasks.len()
        ))
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
                "processing project"
            );
            let roots = enum_map! {
                Root::Workspace => &self.ws_root_dir,
                Root::Project => project.project_dir,
            };

            let mut output_files_globset = None;
            if should_collect_output_files {
                let mut output_glob = GlobSetBuilder::new();

                populate_includes_and_globset(
                    project.output_files,
                    project.project_dir,
                    &mut includes,
                    &roots,
                    &mut output_glob,
                )?;

                output_files_globset = Some(output_glob.build()?);
            }

            let mut input_files_globset = None;

            // the output dirs are based on the hashes so need to be collected here
            if should_collect_input_files {
                let mut input_glob = GlobSetBuilder::new();
                populate_includes_and_globset(
                    project.input_files,
                    project.project_dir,
                    &mut includes,
                    &roots,
                    &mut input_glob,
                )?;

                input_files_globset = Some(input_glob.build()?);
            }

            to_process.push(Holder {
                input_files_globset,
                output_files_globset,
                output_files_glob: project.output_files.to_vec(),
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
                    if self.ws_root_dir.starts_with(parent) {
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
                .build()?
                .build_walker()?;

            for res in dirwalker.walk_dir(&topmost)? {
                let res = res?;
                let original_file_abs_path = res.path();

                if !self.sys.fs_is_file_async(original_file_abs_path).await? {
                    continue;
                }

                for project in &mut to_process {
                    let project_dir = project.roots[Root::Project];
                    let rooted_path = if original_file_abs_path
                        .starts_with(project_dir)
                    {
                        OmniPath::new_rooted(
                            relpath(original_file_abs_path, project_dir),
                            Root::Project,
                        )
                    } else if original_file_abs_path
                        .starts_with(self.ws_root_dir)
                    {
                        OmniPath::new_rooted(
                            relpath(original_file_abs_path, self.ws_root_dir),
                            Root::Workspace,
                        )
                    } else {
                        OmniPath::new(original_file_abs_path)
                    };

                    if let Some(input_files_globset) =
                        project.input_files_globset.as_ref()
                        && input_files_globset.is_match(original_file_abs_path)
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
                        && output_files_globset.is_match(original_file_abs_path)
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
                        task_command: holder.task.task_command,
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

fn populate_includes_and_globset(
    files: &[OmniPath],
    project_dir: &Path,
    includes: &mut Vec<PathBuf>,
    roots: &RootMap,
    output_globset: &mut GlobSetBuilder,
) -> Result<(), Error> {
    trace::trace!(
        files = ?files,
        project_dir = ?project_dir,
        roots = ?roots,
        "populate_includes_and_globset"
    );
    for p in files {
        let p = p.resolve(roots);

        let path = if p.is_relative() {
            std::path::absolute(project_dir.join(p))?
        } else {
            p.to_path_buf()
        };

        let glob = Glob::new(path.to_string_lossy().as_ref())?;
        output_globset.add(glob);
        includes.push(path);
    }
    Ok(())
}
