use std::path::{Path, PathBuf};

use derive_new::new;
use dir_walker::{DirEntry as _, DirWalker as _, impls::RealGlobDirWalker};
use enum_map::enum_map;
use globset::{Glob, GlobSet, GlobSetBuilder};
use omni_types::{OmniPath, Root, RootMap};
use omni_utils::path::{has_globs, relpath, remove_globs, topmost_dirs};
use system_traits::{FsMetadata, FsMetadataAsync, auto_impl, impls::RealSys};

use super::error::Error;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectConfig {
    pub output_files: bool,
    pub input_files: bool,
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
}

#[derive(Debug, new)]
pub struct Collector<'a, TSys: CollectorSys = RealSys> {
    ws_root_dir: &'a Path,
    sys: TSys,
}

struct Holder<'a> {
    output_files_globset: Option<GlobSet>,
    resolved_output_files: Option<Vec<OmniPath>>,
    input_files_globset: Option<GlobSet>,
    resolved_input_files: Option<Vec<OmniPath>>,
    task: ProjectTaskInfo<'a>,
    roots: RootMap<'a>,
}

impl<'a, TSys: CollectorSys> Collector<'a, TSys> {
    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = "debug", skip(self))
    )]
    pub async fn collect(
        &self,
        project_tasks: &[ProjectTaskInfo<'a>],
        config: &CollectConfig,
    ) -> Result<Vec<CollectResult<'a>>, Error> {
        let mut to_process = Vec::with_capacity(project_tasks.len());

        let should_collect_input_files = config.input_files;

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
            trace::debug!(
                project_name = ?project.project_name,
                project = ?project,
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
            });
        }

        if !includes.is_empty() {
            let topmost =
                topmost_dirs(self.sys.clone(), &includes, self.ws_root_dir)
                    .into_iter()
                    .map(|p| p.to_path_buf())
                    .collect::<Vec<_>>();

            let topmost =
                topmost.iter().map(|p| p.as_path()).collect::<Vec<_>>();

            // for some reason, ignore doesn't like it when a folder is ignored
            // and a file inside the ignored folder is included
            // so include all the parent folders of the included file
            // so that ignore walks to it and doesn't ignore it
            let mut forced_includes = Vec::with_capacity(includes.len());

            for include in &includes {
                forced_includes.push(include.to_path_buf());

                let clean = if has_globs(include.to_string_lossy().as_ref()) {
                    let clean = remove_globs(include);

                    forced_includes.push(clean.to_path_buf());

                    clean
                } else {
                    include
                };

                for parent in clean.ancestors() {
                    // if we are in the workspace root, stop here
                    if self.ws_root_dir.starts_with(parent) {
                        break;
                    }

                    forced_includes.push(parent.to_path_buf());
                }
            }

            forced_includes.dedup();

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
                    let rooted_path =
                        if original_file_abs_path.starts_with(project_dir) {
                            OmniPath::new_project_rooted(relpath(
                                original_file_abs_path,
                                project_dir,
                            ))
                        } else if original_file_abs_path
                            .starts_with(self.ws_root_dir)
                        {
                            OmniPath::new_ws_rooted(relpath(
                                original_file_abs_path,
                                self.ws_root_dir,
                            ))
                        } else {
                            OmniPath::new(original_file_abs_path)
                        };

                    if let Some(input_files_globset) =
                        project.input_files_globset.as_ref()
                        && input_files_globset.is_match(original_file_abs_path)
                        && let Some(resolved_input_files) =
                            project.resolved_input_files.as_mut()
                    {
                        resolved_input_files.push(rooted_path.clone());
                    }

                    if let Some(output_files_globset) =
                        project.output_files_globset.as_ref()
                        && output_files_globset.is_match(original_file_abs_path)
                        && let Some(resolved_output_files) =
                            project.resolved_output_files.as_mut()
                    {
                        resolved_output_files.push(rooted_path);
                    }
                }
            }
        }

        Ok(to_process
            .into_iter()
            .map(|p| CollectResult {
                task: p.task,
                input_files: p.resolved_input_files,
                output_files: p.resolved_output_files,
                roots: p.roots,
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
