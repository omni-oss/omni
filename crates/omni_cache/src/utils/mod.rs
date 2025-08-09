use std::path;

pub fn project_dirname(project_name: &str) -> String {
    bs58::encode(project_name).into_string()
}

pub fn relpath<'a>(path: &'a path::Path, base: &path::Path) -> &'a path::Path {
    path.strip_prefix(base)
        .expect("path is not a child of base")
}
