//! This crate is a leaf below `omni_configurations`; depending on it would
//! create a cycle for future plugin configuration crates.
use toml::Table;

const FORBIDDEN: [&str; 2] = ["omni_configurations", "omni_plugin"];
const KINDS: [&str; 3] =
    ["dependencies", "dev-dependencies", "build-dependencies"];

fn collect(table: &Table, workspace: &Table, out: &mut Vec<String>) {
    for kind in KINDS {
        let Some(deps) = table.get(kind).and_then(|v| v.as_table()) else {
            continue;
        };
        for (key, spec) in deps {
            let resolved =
                spec.get("package").and_then(|p| p.as_str()).unwrap_or(key);
            out.push(resolved);
        }
    }
}

fn resolved_dependencies(
    manifest: &str,
    workspace_manifest: &str,
) -> Vec<String> {
    let root: Table = manifest.parse().expect("valid manifest");
    let ws_root: Table = workspace_manifest.parse().expect("valid manifest");
    let workspace = ws_root
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    collect(&root, &workspace, &mut out);
    if let Some(targets) = root.get("target").and_then(|t| t.as_table()) {
        for target in targets.values().filter_map(|t| t.as_table()) {
            collect(target, &workspace, &mut out);
        }
    }
    out
}

fn violations(manifest: &str) -> Vec<String> {
    violations_with(manifest, "")
}

fn violations_with(manifest: &str, workspace_manifest: &str) -> Vec<String> {
    resolved_dependencies(manifest, workspace_manifest)
        .into_iter()
        .filter(|name| FORBIDDEN.iter().any(|f| name.starts_with(f)))
        .collect()
}

#[test]
fn does_not_depend_on_omni_configurations() {
    let manifest = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/Cargo.toml"
    ))
    .unwrap();
    let root = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../Cargo.toml"
    ))
    .unwrap();
    assert_eq!(violations_with(&manifest, &root), Vec::<String>::new());
}

#[test]
fn catches_plain_dependency() {
    let m = "[dependencies]\nomni_configurations = { workspace = true }\n";
    assert_eq!(violations(m), ["omni_configurations"]);
}

#[test]
fn catches_renamed_dependency() {
    let m = "[dependencies]\nconfig = { package = \"omni_configurations\", workspace = true }\n";
    assert_eq!(violations(m), ["omni_configurations"]);
}

#[test]
fn catches_table_style_dependency() {
    let m = "[dependencies.omni_configurations]\nworkspace = true\n";
    assert_eq!(violations(m), ["omni_configurations"]);
}

#[test]
fn catches_dev_build_and_target_tables() {
    let m = "[dev-dependencies]\nomni_plugin_x = \"1\"\n\
             [build-dependencies.omni_configurations]\nworkspace = true\n\
             [target.'cfg(unix)'.dependencies]\nc = { package = \"omni_configurations\", workspace = true }\n";
    assert_eq!(violations(m).len(), 3);
}

#[test]
fn catches_inherited_dependency_renamed_in_workspace() {
    let ws = "[workspace.dependencies]\n\
              cfg = { package = \"omni_configurations\", path = \"x\" }\n";
    let m = "[dev-dependencies]\ncfg = { workspace = true }\n";
    assert_eq!(violations_with(m, ws), ["omni_configurations"]);
}
