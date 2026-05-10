mod error;
mod sys;
use std::path::Path;

pub use error::Error;
use gix::progress::Discard;
pub use sys::GitUtilsSys;

pub async fn clone_repo(
    sys: &impl GitUtilsSys,
    url: &str,
    ref_name: Option<&str>,
    destination: &Path,
) -> Result<String, Error> {
    sys.fs_create_dir_all_async(destination).await?;

    let mut prepare_clone =
        gix::prepare_clone(url, destination).map_err(gix::Error::from_error)?;

    let (mut checkout, _outcome) = prepare_clone
        .fetch_then_checkout(Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(gix::Error::from_error)?;

    let (repo, _) = checkout
        .main_worktree(Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(gix::Error::from_error)?;

    let oid = repo
        .rev_parse_single(ref_name.unwrap_or("HEAD"))
        .map_err(gix::Error::from_error)?
        .detach();
    let commit = repo.find_commit(oid).map_err(gix::Error::from_error)?;
    let tree_id = commit.tree().map_err(gix::Error::from_error)?.id();

    let mut index_state = gix::index::State::from_tree(
        &tree_id,
        &repo.objects,
        Default::default(),
    )
    .map_err(gix::Error::from_error)?;

    repo.reference(
        "HEAD",
        oid,
        gix::refs::transaction::PreviousValue::Any,
        "clone checkout",
    )
    .map_err(gix::Error::from_error)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| eyre::eyre!("repository has no workdir"))?;

    gix::worktree::state::checkout(
        &mut index_state,
        workdir,
        repo.objects.clone(),
        &gix::progress::Discard,
        &gix::progress::Discard,
        &gix::interrupt::IS_INTERRUPTED,
        gix::worktree::state::checkout::Options {
            overwrite_existing: true, // overwrite files from the HEAD checkout
            destination_is_initially_empty: false,
            ..Default::default()
        },
    )
    .map_err(gix::Error::from_error)?;

    Ok(oid.to_string())
}
