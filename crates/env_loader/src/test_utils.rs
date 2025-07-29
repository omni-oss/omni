use system_traits::{
    EnvSetCurrentDir as _, FsCreateDirAll as _, FsWrite as _,
    impls::InMemorySys,
};

use crate::EnvLoaderSys;

pub fn create_sys() -> impl EnvLoaderSys {
    let sys = InMemorySys::default();

    sys.fs_create_dir_all("/root/nested/project")
        .expect("Can't create root dir");

    sys.fs_write("/root/.env", include_str!("../test_fixtures/.env.root"))
        .expect("Can't write root env file");
    sys.fs_write(
        "/root/.env.local",
        include_str!("../test_fixtures/.env.root.local"),
    )
    .expect("Can't write root local env file");

    sys.fs_write(
        "/root/nested/.env",
        include_str!("../test_fixtures/.env.nested"),
    )
    .expect("Can't write nested env file");
    sys.fs_write(
        "/root/nested/.env.local",
        include_str!("../test_fixtures/.env.nested.local"),
    )
    .expect("Can't write nested local env file");
    sys.fs_write(
        "/root/nested/project/.env",
        include_str!("../test_fixtures/.env.project"),
    )
    .expect("Can't write project env file");
    sys.fs_write(
        "/root/nested/project/.env.local",
        include_str!("../test_fixtures/.env.project.local"),
    )
    .expect("Can't write project local env file");
    sys.env_set_current_dir("/root/nested/project")
        .expect("Can't set current dir");

    sys
}

#[macro_export]
macro_rules! env {
    [$($key:expr => $value:expr),*$(,)?] => {{
        let mut hm = std::collections::HashMap::<String, String>::new();
        $(
            hm.insert($key.to_string(), $value.to_string());
        )*
        hm
    }}
}
