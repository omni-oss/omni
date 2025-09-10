use std::path::{Path, PathBuf};

use system_traits::{
    EnvCurrentDir, EnvVars, FsCanonicalize, FsCreateDirAll, FsMetadata,
    FsWrite,
    impls::{InMemorySys, RealSys},
};
use tempfile::TempDir;

pub fn real_sys() -> RealSys {
    RealSys::default()
}

pub fn mem_sys() -> InMemorySys {
    InMemorySys::default()
}

pub fn tmp() -> TempDir {
    let tmp = TempDir::new().expect("can't create temp dir");
    tmp
}

#[system_traits::auto_impl]
pub trait TestSys:
    EnvCurrentDir
    + FsMetadata
    + EnvVars
    + FsWrite
    + FsCanonicalize
    + FsCreateDirAll
    + FsMetadata
    + Clone
    + Send
    + Sync
{
}

pub fn cross_path(p: &str) -> PathBuf {
    if cfg!(windows) && p.contains('/') {
        PathBuf::from(p.replace("/", "\\"))
    } else {
        PathBuf::from(p)
    }
}

pub fn default_fixture() -> (TempDir, RealSys) {
    // wrap it in an Arc so that it doesn't get dropped before the test due to being async
    let tmp = tmp();
    let sys = real_sys();
    setup_fixture(tmp.path(), sys.clone());

    (tmp, sys)
}

pub fn setup_fixture(root: &Path, sys: impl TestSys) {
    sys.fs_create_dir_all(root.join(cross_path("nested/project-1")))
        .expect("Can't create project-1 dir");

    sys.fs_create_dir_all(root.join(cross_path("nested/project-2")))
        .expect("Can't create project-2 dir");
    sys.fs_create_dir_all(root.join(cross_path("nested/project-3")))
        .expect("Can't create project-3 dir");
    sys.fs_create_dir_all(root.join("base"))
        .expect("Can't create project-2 dir");

    sys.fs_write(
        root.join(".env"),
        include_str!("../test_fixtures/.env.root"),
    )
    .expect("Can't write root env file");
    sys.fs_write(
        root.join(".env.local"),
        include_str!("../test_fixtures/.env.root.local"),
    )
    .expect("Can't write root local env file");

    sys.fs_write(
        root.join(cross_path("nested/.env")),
        include_str!("../test_fixtures/.env.nested"),
    )
    .expect("Can't write nested env file");
    sys.fs_write(
        root.join(cross_path("nested/.env.local")),
        include_str!("../test_fixtures/.env.nested.local"),
    )
    .expect("Can't write nested local env file");

    sys.fs_write(
        root.join(cross_path("nested/project-1/.env")),
        include_str!("../test_fixtures/.env.project-1"),
    )
    .expect("Can't write project env file");
    sys.fs_write(
        root.join(cross_path("nested/project-1/.env.local")),
        include_str!("../test_fixtures/.env.project-1.local"),
    )
    .expect("Can't write project local env file");
    sys.fs_write(
        root.join(cross_path("nested/project-1/project.omni.yaml")),
        include_str!("../test_fixtures/project-1.omni.yaml"),
    )
    .expect("Can't write project config file");

    sys.fs_write(
        root.join(cross_path("nested/project-2/.env")),
        include_str!("../test_fixtures/.env.project-2"),
    )
    .expect("Can't write project env file");
    sys.fs_write(
        root.join(cross_path("nested/project-2/.env.local")),
        include_str!("../test_fixtures/.env.project-2.local"),
    )
    .expect("Can't write project local env file");
    sys.fs_write(
        root.join(cross_path("nested/project-2/project.omni.yaml")),
        include_str!("../test_fixtures/project-2.omni.yaml"),
    )
    .expect("Can't write project config file");
    sys.fs_write(
        root.join(cross_path("nested/project-3/project.omni.yaml")),
        include_str!("../test_fixtures/project-3.omni.yaml"),
    )
    .expect("Can't write project config file");

    sys.fs_write(
        root.join(cross_path("workspace.omni.yaml")),
        include_str!("../test_fixtures/workspace.omni.yaml"),
    )
    .expect("Can't write workspace config file");

    sys.fs_write(
        root.join(cross_path("base/base-1.omni.yaml")),
        include_str!("../test_fixtures/base-1.omni.yaml"),
    )
    .expect("Can't write project config file");
    sys.fs_write(
        root.join(cross_path("base/base-2.omni.yaml")),
        include_str!("../test_fixtures/base-2.omni.yaml"),
    )
    .expect("Can't write project config file");
}
