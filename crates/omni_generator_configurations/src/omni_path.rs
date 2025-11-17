#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    enum_map::Enum,
    strum::Display,
    strum::VariantArray,
    strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Root {
    Workspace,
    Output,
}

pub type OmniPath = omni_types::OmniPath<Root>;
