//! Pack expansion tests. They build local pack trees on disk and expand them
//! through a real store; no `git` binary is required because every source is
//! `local`.

use std::path::Path;

use omni_configurations::{PackSourceConfiguration, PackSubsystem};
use omni_remote_source::manager::{
    RemoteSourceManager, config::RemoteSourceConfig,
};
use omni_remote_source_contributors::expand_packs;
use system_traits::impls::RealSys;

async fn manager(sources_root: &Path) -> RemoteSourceManager<RealSys> {
    RemoteSourceManager::new(
        RemoteSourceConfig::builder()
            .lockfile_path(sources_root.join("lock.json"))
            .store_root_path(sources_root.join("store"))
            .build(),
        RealSys,
    )
    .await
    .expect("build manager")
}

fn write_manifest(dir: &Path, contents: &str) {
    std::fs::create_dir_all(dir).expect("create pack dir");
    std::fs::write(dir.join("pack.omni.yaml"), contents)
        .expect("write manifest");
}

fn pack_source(
    id: &str,
    path: &str,
    provides: Option<&str>,
) -> PackSourceConfiguration {
    let provides = provides
        .map(|p| format!(r#","provides":{p}"#))
        .unwrap_or_default();
    let json = format!(
        r#"{{"source":"local","path":"{path}","id":"{id}"{provides}}}"#
    );
    serde_json::from_str(&json).expect("valid pack source")
}

#[tokio::test]
async fn missing_manifest_is_a_hard_error() {
    let ws = tempfile::tempdir().expect("ws");
    let sources_root = ws.path().join(".omni/sources");
    std::fs::create_dir_all(ws.path().join("empty")).expect("empty pack dir");
    let manager = manager(&sources_root).await;

    let sources = vec![pack_source("empty", "./empty", None)];
    let result = expand_packs(&manager, &sources, ws.path(), false, None).await;
    assert!(result.is_err(), "a pack with no manifest must error");
}

#[tokio::test]
async fn both_and_pack_contributes_its_own_sources_and_recurses() {
    let ws = tempfile::tempdir().expect("ws");
    let sources_root = ws.path().join(".omni/sources");

    write_manifest(
        &ws.path().join("a"),
        "name: \"@org/a\"\ngenerators:\n  - source: local\n    path: ./gen\npacks:\n  - source: local\n    path: ./b\n    id: b\n",
    );
    write_manifest(
        &ws.path().join("a/b"),
        "name: \"@org/b\"\ntools:\n  - source: local\n    path: ./tools\n",
    );

    let manager = manager(&sources_root).await;
    let sources = vec![pack_source("a", "./a", None)];
    let (packs, _refs) =
        expand_packs(&manager, &sources, ws.path(), false, None)
            .await
            .unwrap();

    assert_eq!(packs.nodes.len(), 2, "the pack and its sub-pack");
    let gens = packs.effective_generator_sources();
    let tools = packs.effective_tool_sources();
    assert_eq!(gens.len(), 1, "the top pack contributes a generator source");
    assert_eq!(gens[0].qualified_id, "a");
    assert_eq!(tools.len(), 1, "the sub-pack contributes a tool source");
    assert_eq!(tools[0].qualified_id, "a::b");
}

#[tokio::test]
async fn a_pack_without_projections_contributes_no_projection_face() {
    let ws = tempfile::tempdir().expect("ws");
    let sources_root = ws.path().join(".omni/sources");
    write_manifest(
        &ws.path().join("a"),
        "name: \"@org/a\"\ngenerators:\n  - source: local\n    path: ./gen\n",
    );

    let manager = manager(&sources_root).await;
    let sources = vec![pack_source("a", "./a", None)];
    let (packs, _refs) =
        expand_packs(&manager, &sources, ws.path(), false, None)
            .await
            .unwrap();

    assert!(
        packs.effective_projection_sources().is_empty(),
        "a pack without a projections list has no projection face"
    );
}

#[tokio::test]
async fn provides_narrows_and_cascades_and_never_widens() {
    let ws = tempfile::tempdir().expect("ws");
    let sources_root = ws.path().join(".omni/sources");

    // Top pack A provides only generators. Its sub-pack B declares both
    // generators and projections, and the entry to B asks for projections too.
    write_manifest(
        &ws.path().join("a"),
        "name: \"@org/a\"\ngenerators:\n  - source: local\n    path: ./gen\npacks:\n  - source: local\n    path: ./b\n    id: b\n    provides: [generators, projections]\n",
    );
    write_manifest(
        &ws.path().join("a/b"),
        "name: \"@org/b\"\ngenerators:\n  - source: local\n    path: ./gen\nprojections:\n  - source: local\n    path: ./skills\n    id: skills\n    routes: [{ strategy: namespaced }]\n",
    );

    let manager = manager(&sources_root).await;
    let sources = vec![pack_source("a", "./a", Some(r#"["generators"]"#))];
    let (packs, _refs) =
        expand_packs(&manager, &sources, ws.path(), false, None)
            .await
            .unwrap();

    let b = packs
        .nodes
        .iter()
        .find(|n| n.qualified_id == "a::b")
        .expect("sub-pack present");
    let provides = b.effective_provides.as_ref().expect("narrowed");
    assert!(provides.contains(&PackSubsystem::Generators));
    assert!(
        !provides.contains(&PackSubsystem::Projections),
        "the ancestor's generators-only gate caps the sub-pack; provides cannot widen"
    );

    assert!(
        packs.effective_projection_sources().is_empty(),
        "no projection face survives the cascade"
    );
    assert_eq!(
        packs.effective_generator_sources().len(),
        2,
        "both packs contribute generators"
    );
}

const SKILLS_PACK: &str = "name: \"@org/kit\"\nprojections:\n  - source: local\n    path: ./skills\n    id: skills\ngenerators:\n  - source: local\n    path: ./gens\npacks:\n  - source: local\n    path: ./inner\n    id: inner\n";
const INNER_PACK: &str =
    "name: \"@org/inner\"\ntools:\n  - source: local\n    path: ./tools\n";

#[tokio::test]
async fn packs_reached_from_the_workspace_alone_are_local() {
    let ws = tempfile::tempdir().expect("ws");
    let sources_root = ws.path().join(".omni/sources");
    write_manifest(&ws.path().join("kit"), SKILLS_PACK);
    write_manifest(&ws.path().join("kit/inner"), INNER_PACK);

    let manager = manager(&sources_root).await;
    let sources = vec![pack_source("kit", "./kit", None)];
    let (packs, _refs) =
        expand_packs(&manager, &sources, ws.path(), false, None)
            .await
            .unwrap();

    assert_eq!(packs.nodes.len(), 2);
    assert!(packs.nodes.iter().all(|n| n.is_local));
    assert!(
        packs
            .effective_generator_sources()
            .iter()
            .all(|s| s.is_local)
    );
    assert!(packs.effective_tool_sources().iter().all(|s| s.is_local));
    assert!(
        packs.non_local_projection_ids().is_empty(),
        "no projection of a local pack is marked non-local"
    );
}

/// A workspace-local pack may be configured to point into the store. It was
/// reached from the workspace alone, so it and its local children stay local
/// and keep the workspace's ignore rules.
#[tokio::test]
async fn a_workspace_local_pack_pointing_into_the_store_keeps_its_children_local()
 {
    let ws = tempfile::tempdir().expect("ws");
    let sources_root = ws.path().join(".omni/sources");
    let in_store = sources_root.join("store/manual/kit");
    write_manifest(&in_store, SKILLS_PACK);
    write_manifest(&in_store.join("inner"), INNER_PACK);

    let manager = manager(&sources_root).await;
    let sources =
        vec![pack_source("kit", "./.omni/sources/store/manual/kit", None)];
    let (packs, _refs) =
        expand_packs(&manager, &sources, ws.path(), false, None)
            .await
            .unwrap();

    assert_eq!(packs.nodes.len(), 2);
    assert!(
        packs.nodes.iter().all(|n| n.is_local),
        "locality follows how the pack was reached, not where its files are"
    );
    assert!(packs.effective_tool_sources().iter().all(|s| s.is_local));
    assert!(packs.non_local_projection_ids().is_empty());
}

/// Pack ids are free-form strings, so a child id may contain the `::` that joins
/// qualified ids. Locality is resolved through the real parent, not by parsing
/// the qualified id, so such a child of a local pack stays local.
#[tokio::test]
async fn a_child_id_containing_the_separator_keeps_its_parents_locality() {
    let ws = tempfile::tempdir().expect("ws");
    let sources_root = ws.path().join(".omni/sources");
    write_manifest(
        &ws.path().join("kit"),
        "name: \"@org/kit\"\npacks:\n  - source: local\n    path: ./inner\n    id: \"inner::leaf\"\n",
    );
    write_manifest(&ws.path().join("kit/inner"), INNER_PACK);

    let manager = manager(&sources_root).await;
    let sources = vec![pack_source("kit", "./kit", None)];
    let (packs, _refs) =
        expand_packs(&manager, &sources, ws.path(), false, None)
            .await
            .unwrap();

    assert_eq!(packs.nodes.len(), 2);
    assert!(packs.nodes.iter().all(|n| n.is_local));
    assert!(packs.effective_tool_sources().iter().all(|s| s.is_local));
}
