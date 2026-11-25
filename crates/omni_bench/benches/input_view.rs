//! Overhead of the captured input view (RFC 0024 F10.2b) against plain reads.
//!
//! - `tree_1000`: 1,000 files in 10 directories, 2 KiB each: read every file
//!   and list every directory, plain `std::fs` against a fresh `InputView`.
//! - `lockfiles`: real pnpm and yarn lockfiles, read plain against through the
//!   view. Set `OMNI_BENCH_LOCKFILES` to a directory holding them; the group is
//!   skipped when it is unset. Used to calibrate the default `InputLimits`.
//!
//! The view is only built for Linux and Windows (macOS is `Unsupported`).
#![allow(clippy::disallowed_macros)]

#[cfg(any(target_os = "linux", windows))]
mod real {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use criterion::{BenchmarkId, Criterion, Throughput, criterion_group};
    use omni_input_view::{
        AuthDecision, AuthError, Cancellation, InputAuthorizer, InputLimits,
        InputView, InputViewOptions, SymlinkPolicy,
    };
    use omni_path_utils::WorkspacePath;

    struct AllowAll;

    impl InputAuthorizer for AllowAll {
        fn authorize_read(
            &self,
            _: &WorkspacePath,
        ) -> Result<AuthDecision, AuthError> {
            Ok(AuthDecision::Allow)
        }
    }

    fn view(root: &Path, limits: InputLimits) -> InputView {
        InputView::open(
            root,
            InputViewOptions {
                authorizer: Arc::new(AllowAll),
                limits,
                symlinks: SymlinkPolicy::Reject,
            },
        )
        .expect("open view")
    }

    fn tree_1000(criterion: &mut Criterion) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let body = vec![b'x'; 2048];
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        for dir in 0..10 {
            let name = format!("dir{dir}");
            fs::create_dir(root.join(&name)).unwrap();
            for file in 0..100 {
                let file_name = format!("file{file}.txt");
                fs::write(root.join(&name).join(&file_name), &body).unwrap();
                files.push((
                    root.join(&name).join(&file_name),
                    format!("{name}/{file_name}"),
                ));
            }
            dirs.push((root.join(&name), name));
        }
        let paths: Vec<WorkspacePath> = files
            .iter()
            .map(|(_, rel)| WorkspacePath::parse(rel).unwrap())
            .collect();
        let dir_paths: Vec<WorkspacePath> = dirs
            .iter()
            .map(|(_, rel)| WorkspacePath::parse(rel).unwrap())
            .collect();
        let cx = Cancellation::new();

        let mut group = criterion.benchmark_group("tree_1000");
        group.throughput(Throughput::Elements(files.len() as u64));
        group.bench_function("read/plain", |bench| {
            bench.iter(|| {
                let mut total = 0usize;
                for (path, _) in &files {
                    total += fs::read(path).unwrap().len();
                }
                total
            });
        });
        group.bench_function("read/view", |bench| {
            bench.iter(|| {
                let view = view(&root, InputLimits::default());
                let mut total = 0usize;
                for path in &paths {
                    if let omni_input_view::Read::Found(bytes) =
                        view.read(path, &cx).unwrap()
                    {
                        total += bytes.len();
                    }
                }
                total
            });
        });
        group.bench_function("list/plain", |bench| {
            bench.iter(|| {
                let mut total = 0usize;
                for (path, _) in &dirs {
                    total += fs::read_dir(path).unwrap().count();
                }
                total
            });
        });
        group.bench_function("list/view", |bench| {
            bench.iter(|| {
                let view = view(&root, InputLimits::default());
                let mut total = 0usize;
                for path in &dir_paths {
                    if let omni_input_view::List::Found(listing) =
                        view.list(path, &cx).unwrap()
                    {
                        total += listing.entries.len();
                    }
                }
                total
            });
        });
        group.finish();
    }

    fn lockfiles(criterion: &mut Criterion) {
        let Some(dir) =
            std::env::var_os("OMNI_BENCH_LOCKFILES").map(PathBuf::from)
        else {
            return;
        };
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .expect("read OMNI_BENCH_LOCKFILES")
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        entries.sort();
        let cx = Cancellation::new();
        let mut group = criterion.benchmark_group("lockfiles");
        for name in entries {
            let size = fs::metadata(dir.join(&name)).unwrap().len();
            group.throughput(Throughput::Bytes(size));
            let path = WorkspacePath::parse(&name).unwrap();
            group.bench_with_input(
                BenchmarkId::new("plain", &name),
                &name,
                |bench, name| {
                    bench.iter(|| fs::read(dir.join(name)).unwrap().len())
                },
            );
            group.bench_with_input(
                BenchmarkId::new("view", &name),
                &name,
                |bench, _| {
                    bench.iter(|| {
                        // Fresh view each time so the first-capture path is timed.
                        let view = view(&dir, InputLimits::default());
                        match view.read(&path, &cx).unwrap() {
                            omni_input_view::Read::Found(bytes) => bytes.len(),
                            omni_input_view::Read::Missing => 0,
                        }
                    });
                },
            );
        }
        group.finish();
    }

    criterion_group!(benches, tree_1000, lockfiles);
    pub fn run() {
        benches();
        Criterion::default().configure_from_args().final_summary();
    }
}

fn main() {
    #[cfg(any(target_os = "linux", windows))]
    real::run();
}
