use garde::Validate;
use omni_config_types::SingleOrMany;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

/// Assigns each [`SourceConfig`] variant its own extra family plus one shared
/// base that every variant carries.
///
/// A profile is a zero-sized marker that selects the extra families flattened
/// into each source variant, mirroring the `InputProfile` pattern. `()` is the
/// null profile used by generators and tools, which carry no extra fields; the
/// serialized shape of a `SourceConfig<()>` is exactly the bare
/// `source`/`path`/`uri`/`rev` object.
pub trait SourceConfigProfile: Default + Clone + Sized {
    /// Fields identical across every source kind (e.g. a pack's `provides`, a
    /// projection's inline `routes`). Profiles that need nothing here use `()`.
    type BaseExtra: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + Default
        + Send
        + Sync;

    /// Extra fields carried only by a `local` source.
    type LocalExtra: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + Default
        + Send
        + Sync;

    /// Extra fields carried only by a `git` source.
    type GitExtra: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + Default
        + Send
        + Sync;

    /// Extra fields carried only by the (serde-hidden) `registry` source.
    type RegistryExtra: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + Default
        + Send
        + Sync;

    /// The declaration-site identity handle, if this source kind carries one.
    ///
    /// Uniform accessor over the per-variant extras: `git`/`local` carry a
    /// required `id`; the registry variant an optional one. Generators and
    /// tools are identity-less and return `None`.
    fn declared_id(source: &SourceConfig<Self>) -> Option<&str>;
}

/// A registered source of configuration artifacts (generator manifests, tool
/// manifests, projection inputs, or packs).
///
/// The type is generic over a [`SourceConfigProfile`] that assigns each variant
/// its own flattened extra family. `P = ()` yields the plain `local`/`git`
/// source shared by generators and tools; richer profiles add sibling fields
/// without changing the wire shape of the `source`/`path`/`uri`/`rev` keys.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Validate,
)]
#[serde(
    tag = "source",
    rename_all = "kebab-case",
    deny_unknown_fields,
    bound(deserialize = "", serialize = "")
)]
#[schemars(bound(deserialize = "", serialize = ""))]
#[garde(allow_unvalidated)]
pub enum SourceConfig<P: SourceConfigProfile = ()> {
    Local(LocalSource<P>),
    Git(GitSource<P>),
    /// Inert stub for a future package registry. Never constructed today and
    /// hidden from both the wire format and the published schema. It exists so
    /// the future registry is a pure addition: [`SourceConfigProfile::RegistryExtra`]
    /// already gives `id` a place to be optional here while it is required on
    /// `git`/`local`, and the locator fields already follow the `git` pattern.
    #[serde(skip)]
    #[schemars(skip)]
    Registry(RegistrySource<P>),
}

impl<P: SourceConfigProfile> SourceConfig<P> {
    /// The shared base extras carried by every variant.
    pub fn base(&self) -> &P::BaseExtra {
        match self {
            SourceConfig::Local(local) => &local.base,
            SourceConfig::Git(git) => &git.base,
            SourceConfig::Registry(registry) => &registry.base,
        }
    }

    /// The declaration-site identity handle, if this source kind carries one.
    pub fn declared_id(&self) -> Option<&str> {
        P::declared_id(self)
    }
}

/// A `local` source: one or more workspace-relative paths.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Validate,
)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
#[garde(allow_unvalidated)]
pub struct LocalSource<P: SourceConfigProfile = ()> {
    pub path: SingleOrMany<String>,

    #[serde(flatten)]
    #[garde(skip)]
    pub base: P::BaseExtra,

    #[serde(flatten)]
    #[garde(skip)]
    pub extra: P::LocalExtra,
}

/// A `git` source: a repository URI pinned to a revision.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Validate,
)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
#[garde(allow_unvalidated)]
pub struct GitSource<P: SourceConfigProfile = ()> {
    pub uri: Url,

    pub rev: String,

    #[serde(flatten)]
    #[garde(skip)]
    pub base: P::BaseExtra,

    #[serde(flatten)]
    #[garde(skip)]
    pub extra: P::GitExtra,
}

/// A `registry` source: a package name pinned to a version. See the
/// [`SourceConfig::Registry`] doc comment for why this inert stub exists.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Validate,
)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
#[garde(allow_unvalidated)]
pub struct RegistrySource<P: SourceConfigProfile = ()> {
    pub name: String,

    #[serde(default)]
    pub version: Option<String>,

    #[serde(flatten)]
    #[garde(skip)]
    pub base: P::BaseExtra,

    #[serde(flatten)]
    #[garde(skip)]
    pub extra: P::RegistryExtra,
}

/// The null profile: every extra family is `()`, so a `SourceConfig<()>` has no
/// fields beyond `source`/`path`/`uri`/`rev` and carries no identity.
impl SourceConfigProfile for () {
    type BaseExtra = ();
    type LocalExtra = ();
    type GitExtra = ();
    type RegistryExtra = ();

    fn declared_id(_source: &SourceConfig<Self>) -> Option<&str> {
        None
    }
}

/// Generator sources have no extra fields.
pub type GeneratorSourceConfiguration = SourceConfig<()>;

/// Tool sources have no extra fields; the serialized shape is identical to
/// [`GeneratorSourceConfiguration`].
pub type ToolSourceConfiguration = SourceConfig<()>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_source_round_trips_legacy_wire_shape() {
        let json = r#"{"source":"local","path":"./tools"}"#;
        let parsed: SourceConfig = serde_json::from_str(json).expect("valid");
        assert_eq!(
            parsed,
            SourceConfig::Local(LocalSource {
                path: SingleOrMany::Single("./tools".to_string()),
                base: (),
                extra: (),
            })
        );
        assert_eq!(serde_json::to_string(&parsed).expect("serialize"), json);
    }

    #[test]
    fn git_source_round_trips_legacy_wire_shape() {
        let json = r#"{"source":"git","uri":"https://example.com/a.git","rev":"main"}"#;
        let parsed: SourceConfig = serde_json::from_str(json).expect("valid");
        assert_eq!(serde_json::to_string(&parsed).expect("serialize"), json);
    }

    #[test]
    fn git_source_rejects_unknown_key() {
        let json = r#"{"source":"git","uri":"https://example.com/a.git","rev":"main","typo":1}"#;
        let result = serde_json::from_str::<SourceConfig>(json);
        assert!(result.is_err(), "unknown key must be rejected");
    }

    #[test]
    fn local_source_rejects_unknown_key() {
        let json = r#"{"source":"local","path":"./x","typo":1}"#;
        let result = serde_json::from_str::<SourceConfig>(json);
        assert!(result.is_err(), "unknown key must be rejected");
    }

    #[test]
    fn base_and_declared_id_for_the_null_profile() {
        let local: SourceConfig = SourceConfig::Local(LocalSource {
            path: SingleOrMany::Single("./x".to_string()),
            base: (),
            extra: (),
        });
        assert_eq!(local.base(), &());
        assert_eq!(local.declared_id(), None);

        let git: SourceConfig = SourceConfig::Git(GitSource {
            uri: Url::parse("https://example.com/a.git").unwrap(),
            rev: "main".to_string(),
            base: (),
            extra: (),
        });
        assert_eq!(git.base(), &());
        assert_eq!(git.declared_id(), None);
    }
}
