use git2::Repository;

use crate::{ScmImplementation, SelectScm, SupportedScm, git::Git};

#[inline(always)]
fn detect_scm(path: &str) -> Option<SupportedScm> {
    if git2::Repository::discover(path).ok().is_some() {
        return Some(SupportedScm::Git);
    }

    None
}

#[inline(always)]
fn get_supported_impl(
    path: &str,
    scm: SupportedScm,
) -> Option<ScmImplementation> {
    Some(match scm {
        SupportedScm::Git => ScmImplementation::new_git(Git::new(
            Repository::discover(path).ok()?,
        )),
    })
}

#[inline(always)]
fn to_supported_scm(scm: SelectScm) -> Option<SupportedScm> {
    match scm {
        SelectScm::Auto | SelectScm::None => None,
        SelectScm::Git => Some(SupportedScm::Git),
    }
}

#[inline(always)]
pub fn get_scm_implementation(
    path: &str,
    selected_scm: SelectScm,
) -> Option<ScmImplementation> {
    Some(match selected_scm {
        SelectScm::Auto => {
            let detected = detect_scm(path)?;

            get_supported_impl(path, detected)?
        }
        SelectScm::None => return None,
        _ => get_supported_impl(path, to_supported_scm(selected_scm)?)?,
    })
}
