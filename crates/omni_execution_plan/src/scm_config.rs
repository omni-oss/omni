use derive_new::new;
use omni_scm::SelectScm;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, new)]
pub struct ScmAffectedFilter {
    pub scm: SelectScm,
    pub base: Option<String>,
    pub target: Option<String>,
}
