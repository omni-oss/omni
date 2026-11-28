//! Notices raised at the composition boundary.

use std::collections::BTreeSet;

use omni_notice::Notice;

use crate::{Composed, FieldOverride};

omni_codes::prefixes!("composition");

/// Plugins overrode each other's configuration.
///
/// A later plugin in the activation list set a tracked field to a value that
/// differs from an earlier plugin's, and no explicit configuration decides that
/// field. The later plugin's value is used. Counts only: which fields and
/// plugins are involved is in inspection output (`cache.enabled`) and, for
/// other fields, not exposed yet.
///
/// Fix: nothing is wrong. To pick a value yourself, set the field in your
/// project configuration, or change the plugin order.
#[derive(Notice, Debug, Clone, Copy, PartialEq, Eq)]
#[notice(
    code = "composition.plugin_override",
    level = info,
    message = "{fields} field(s) in {projects} project(s) were overridden by a later plugin"
)]
pub struct PluginOverride {
    pub fields: usize,
    pub projects: usize,
}

impl PluginOverride {
    /// Aggregate override records of any number of projects into one notice.
    /// Records masked by an explicit layer or without an overridden
    /// contributor do not count. `None` when nothing counts.
    pub fn from_overrides<'a>(
        overrides: impl IntoIterator<Item = &'a FieldOverride>,
    ) -> Option<Self> {
        let mut fields = BTreeSet::new();
        let mut projects = BTreeSet::new();
        for record in overrides {
            if record.masked_by_explicit || record.overridden.is_empty() {
                continue;
            }
            fields.insert((&record.root, &record.field));
            projects.insert(&record.root);
        }
        (!fields.is_empty()).then_some(Self {
            fields: fields.len(),
            projects: projects.len(),
        })
    }
}

impl Composed {
    /// The notice for this one project's composition, if any field counts.
    /// Reads the records only; composition is unchanged.
    pub fn override_notice(&self) -> Option<PluginOverride> {
        PluginOverride::from_overrides(&self.overrides)
    }
}
