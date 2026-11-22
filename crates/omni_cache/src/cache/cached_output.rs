use std::path::PathBuf;

use derive_new::new;
use omni_hasher::impls::DefaultHash;
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    new,
)]
#[allow(clippy::too_many_arguments)]
pub struct CachedTaskExecution {
    #[new(into)]
    pub project_name: String,

    #[new(into)]
    pub task_name: String,

    #[new(into)]
    #[serde(alias = "task_command")]
    pub task_exec: Option<String>,

    #[new(into)]
    #[serde(alias = "task_retry_command")]
    pub task_retry_exec: Option<String>,

    #[new(into)]
    #[serde(alias = "execution_hash")]
    pub digest: DefaultHash,

    #[new(into)]
    #[serde(alias = "dependency_hashes")]
    pub dependency_digests: Vec<DefaultHash>,

    #[new(into)]
    /// Location of the cached output
    pub logs_path: Option<PathBuf>,

    #[new(into)]
    pub files: Vec<CachedFileOutput>,

    #[new(into)]
    pub exit_code: u32,

    #[new(into)]
    pub execution_duration: std::time::Duration,

    #[new(into)]
    pub execution_time: OffsetDateTime,

    #[new(into)]
    #[serde(default = "default_tries")]
    pub tries: u16,

    /// How the paths in this entry are encoded; see [`PATH_FORMAT_PORTABLE`].
    ///
    /// Entry metadata is positional MessagePack (a struct is an array), so
    /// this field must stay last and fields are only ever appended. Older
    /// binaries reject the longer array and treat the entry as a miss;
    /// entries written before the field existed decode to
    /// [`PATH_FORMAT_LEGACY`] through `serde(default)`. Do not add
    /// `skip_serializing_if` here: it would change the array length.
    #[new(value = "PATH_FORMAT_PORTABLE")]
    #[serde(default)]
    pub path_format: u8,
}

/// Entries written before the marker existed. Rooted paths use the writer's
/// native separator, so a backslash is ambiguous off Windows.
pub const PATH_FORMAT_LEGACY: u8 = 0;

/// Rooted paths (`original_path`) use `/` as the only separator on every OS.
/// A backslash is an ordinary filename character, which is invalid on Windows.
pub const PATH_FORMAT_PORTABLE: u8 = 1;

/// True when `path`, an entry-relative or root-relative path from an entry
/// with the given `format`, may be used on the host OS.
///
/// A marked path accepts `/` only, so a backslash is a literal name off
/// Windows and an invalid name on Windows. An unmarked path is accepted on
/// Windows with either separator; elsewhere a backslash is ambiguous (a
/// Windows separator or a literal name) and is refused rather than guessed.
pub fn path_format_allows(format: u8, path: &std::path::Path) -> bool {
    path_format_allows_on(format, path, cfg!(windows))
}

/// [`path_format_allows`] for an explicit host, so each side can be tested
/// on any OS.
pub fn path_format_allows_on(
    format: u8,
    path: &std::path::Path,
    windows: bool,
) -> bool {
    let has_backslash = path.as_os_str().as_encoded_bytes().contains(&b'\\');
    match format {
        PATH_FORMAT_PORTABLE => !(has_backslash && windows),
        PATH_FORMAT_LEGACY => !(has_backslash && !windows),
        _ => false,
    }
}

impl CachedTaskExecution {
    /// True when every path in the entry can be used on the host OS (see
    /// [`path_format_allows`]). An unknown `path_format` is never valid.
    pub fn paths_match_host_format(&self) -> bool {
        self.paths_match_format_on(cfg!(windows))
    }

    /// True for a marker this version understands. An unknown one comes from
    /// a newer writer, so even an entry with no paths is not usable.
    pub fn has_known_path_format(&self) -> bool {
        matches!(self.path_format, PATH_FORMAT_LEGACY | PATH_FORMAT_PORTABLE)
    }

    fn paths_match_format_on(&self, windows: bool) -> bool {
        let ok = |p: &std::path::Path| {
            path_format_allows_on(self.path_format, p, windows)
        };
        // Check the marker first: an entry with no logs and no files would
        // otherwise pass without `ok` ever running on an unknown marker.
        self.has_known_path_format()
            && self.logs_path.as_deref().is_none_or(ok)
            && self.files.iter().all(|f| {
                ok(&f.cached_path) && ok(f.original_path.unresolved_path())
            })
    }
}

#[inline(always)]
fn default_tries() -> u16 {
    1
}

#[derive(
    Debug,
    Copy,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    new,
)]
pub struct CachedTaskExecutionHash<'a> {
    #[new(into)]
    pub project_name: &'a str,

    #[new(into)]
    pub task_name: &'a str,

    #[new(into)]
    pub digest: DefaultHash,
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    new,
)]
pub struct CachedFileOutput {
    /// Canonical path to the cached file
    #[new(into)]
    pub cached_path: PathBuf,

    /// Canonical path to the original file
    #[new(into)]
    pub original_path: OmniPath,
}
