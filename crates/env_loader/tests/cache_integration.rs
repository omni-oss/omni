use std::path::Path;

use env_loader::{DefaultEnvCache, EnvCache as _, EnvConfig};

use system_traits::{
    EnvSetCurrentDir as _, FsCreateDirAll as _, FsWrite as _,
    impls::InMemorySys,
};

pub fn create_sys() -> InMemorySys {
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

#[test]
fn test_cache_integration() {
    let sys = create_sys();
    let mut cache = DefaultEnvCache::new(sys.clone());

    let first_call = env_loader::load_with_caching(
        &EnvConfig {
            start_dir: Some(Path::new("/root/nested/project")),
            ..Default::default()
        },
        sys.clone(),
        Some(&mut cache),
    )
    .expect("Can't load env");

    let second_call = env_loader::load_with_caching(
        &EnvConfig {
            start_dir: Some(Path::new("/root/nested/project")),
            ..Default::default()
        },
        sys.clone(),
        Some(&mut cache),
    )
    .expect("Can't load env");

    assert!(cache.is_cached(Path::new("."),), "Should be cached");
    assert!(
        cache.is_cached(Path::new("/root/nested"),),
        "Should be cached"
    );
    assert!(cache.is_cached(Path::new("/root"),), "Should be cached");
    assert_eq!(first_call, second_call, "Should be the same");
}

#[test]
fn test_integration_using_relative_paths() {
    let sys = create_sys();
    let mut cache = DefaultEnvCache::new(sys.clone());

    _ = env_loader::load_with_caching(
        &EnvConfig {
            start_dir: Some(Path::new(".")),
            ..Default::default()
        },
        sys.clone(),
        Some(&mut cache),
    )
    .expect("Can't load env");

    assert!(cache.is_cached(Path::new("."),), "Should be cached");
    assert!(cache.is_cached(Path::new("../"),), "Should be cached");
    assert!(cache.is_cached(Path::new("../../"),), "Should be cached");
}
