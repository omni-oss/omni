//! Where a declaration came from, kept beside the values it explains.
//!
//! Provenance is replay metadata, not configuration: it is never part of an
//! authored schema or a serialized configuration.

use std::sync::Arc;

use omni_types::OmniPath;

/// A stable identity for a declaring layer that is not a file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(Arc<str>);

impl LayerId {
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The stable identifier of an ordered contributor (an activation). It names
/// the contributor in provenance; it never orders contributors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContributorId(Arc<str>);

impl ContributorId {
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What declared a value. The kind is explicit rather than inferred from an
/// optional file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationSource {
    File {
        path: Arc<OmniPath>,
    },
    Fileless {
        id: LayerId,
    },
    /// An ordered contributor's own emitted declaration (plugin tier).
    Contributor {
        id: ContributorId,
    },
}

impl Default for DeclarationSource {
    /// The anonymous fileless source: a declaration nothing can attribute.
    fn default() -> Self {
        Self::Fileless {
            id: LayerId::default(),
        }
    }
}

impl DeclarationSource {
    pub fn file(path: OmniPath) -> Self {
        Self::File {
            path: Arc::new(path),
        }
    }

    pub fn contributor(id: ContributorId) -> Self {
        Self::Contributor { id }
    }

    /// The declaring contributor, if the source is one.
    pub fn contributor_id(&self) -> Option<&ContributorId> {
        match self {
            Self::Contributor { id } => Some(id),
            Self::File { .. } | Self::Fileless { .. } => None,
        }
    }

    /// The declaring file, if the source is one.
    pub fn file_path(&self) -> Option<&Arc<OmniPath>> {
        match self {
            Self::File { path } => Some(path),
            Self::Fileless { .. } | Self::Contributor { .. } => None,
        }
    }
}

/// A declaration's source and the directory its relative paths resolve from.
/// The base is a path-resolution input, not part of the source's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub source: DeclarationSource,
    pub base_dir: OmniPath,
}

impl Origin {
    pub fn new(source: DeclarationSource, base_dir: OmniPath) -> Self {
        Self { source, base_dir }
    }

    /// The origin of a declaration that was composed without any attribution.
    pub fn unattributed() -> Self {
        Self::new(DeclarationSource::default(), OmniPath::default())
    }
}

/// A tracked value with the origin that declared it. They only change
/// together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared<T> {
    pub value: T,
    pub origin: Arc<Origin>,
}

/// Replace the slot when `incoming` is present; an absent value changes
/// nothing, so a lower layer's declaration survives.
pub fn replace_if_present<T>(
    slot: &mut Option<Declared<T>>,
    incoming: Option<T>,
    origin: &Arc<Origin>,
) {
    if let Some(value) = incoming {
        *slot = Some(Declared {
            value,
            origin: Arc::clone(origin),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(file: &str) -> Arc<Origin> {
        Arc::new(Origin::new(
            DeclarationSource::file(OmniPath::new(file)),
            OmniPath::new("dir"),
        ))
    }

    #[test]
    fn absent_incoming_keeps_the_slot() {
        let mut slot = None;
        replace_if_present(&mut slot, Some(true), &origin("a.yaml"));
        replace_if_present(&mut slot, None, &origin("b.yaml"));
        let declared = slot.unwrap();
        assert!(declared.value);
        assert_eq!(
            declared.origin.source.file_path().unwrap().path().unwrap(),
            std::path::Path::new("a.yaml")
        );
    }

    #[test]
    fn an_equal_value_still_takes_the_new_origin() {
        let mut slot = None;
        replace_if_present(&mut slot, Some(true), &origin("a.yaml"));
        replace_if_present(&mut slot, Some(true), &origin("b.yaml"));
        assert_eq!(
            slot.unwrap()
                .origin
                .source
                .file_path()
                .unwrap()
                .path()
                .unwrap(),
            std::path::Path::new("b.yaml")
        );
    }

    #[test]
    fn a_fileless_winner_has_an_origin_but_no_file() {
        let mut slot = None;
        replace_if_present(&mut slot, Some(true), &origin("a.yaml"));
        let fileless = Arc::new(Origin::new(
            DeclarationSource::Fileless {
                id: LayerId::new("frontend"),
            },
            OmniPath::default(),
        ));
        replace_if_present(&mut slot, Some(false), &fileless);
        let declared = slot.unwrap();
        assert!(!declared.value);
        assert!(declared.origin.source.file_path().is_none());
    }

    #[test]
    fn a_contributor_winner_has_an_origin_but_no_file() {
        let mut slot = None;
        replace_if_present(&mut slot, Some(true), &origin("a.yaml"));
        let contributor = Arc::new(Origin::new(
            DeclarationSource::contributor(ContributorId::new("native")),
            OmniPath::default(),
        ));
        replace_if_present(&mut slot, Some(false), &contributor);
        let declared = slot.unwrap();
        assert!(!declared.value);
        assert!(declared.origin.source.file_path().is_none());
        assert_eq!(
            declared.origin.source.contributor_id().unwrap().as_str(),
            "native"
        );
    }

    #[test]
    fn contributor_identity_is_not_a_file_or_an_anonymous_fileless_id() {
        let contributor =
            DeclarationSource::contributor(ContributorId::new("a"));
        assert_ne!(
            contributor,
            DeclarationSource::Fileless {
                id: LayerId::new("a")
            }
        );
        assert_ne!(contributor, DeclarationSource::default());
        assert!(
            DeclarationSource::file(OmniPath::new("a"))
                .contributor_id()
                .is_none()
        );
    }
}
