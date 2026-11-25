//! Final-pass revalidation (F10.3, D3, D6, D8).
//!
//! Every tracked path is looked at again through the same containment and
//! authorization path as the original access, with the content re-read and
//! re-digested (file identity and timestamps are never trusted). The pass is
//! read-only: evidence and captured bytes stay untouched, so it can be run
//! twice or cancelled.
//!
//! Honest guarantee (RFC 0024 section 17.2): a change is detected only if it
//! happened before the last re-observation of that path. An edit after it, or
//! during the pass itself, is not detected, and no test asserts otherwise.

use super::*;
use crate::evidence::PathEvidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Same kind, different content.
    Modified,
    /// Was absent, now exists (any kind).
    Created,
    /// Was present, now gone. Also a case-only rename of the tracked name
    /// (D8: the exact spelling no longer exists; a name tracked as absent that
    /// now exists with another spelling is `Created`) and a parent that became a
    /// file (D6).
    Deleted,
    /// File became a directory or the reverse, or something else.
    KindChanged,
    /// A directory's authorized membership differs.
    MembershipChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: WorkspacePath,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revalidation {
    Unchanged,
    /// `changes` holds at most `max_reported_changes`, in path order;
    /// `remainder` counts the changes that were detected but not listed.
    Changed {
        changes: Vec<Change>,
        remainder: u64,
    },
}

/// The default consumer policy: any change rejects the pass. There is no
/// retry or restart here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("inputs changed during the pass ({} listed, {remainder} more)", changes.len())]
pub struct InputsChanged {
    pub changes: Vec<Change>,
    pub remainder: u64,
}

impl Revalidation {
    pub fn is_changed(&self) -> bool {
        matches!(self, Self::Changed { .. })
    }

    pub fn reject_on_change(self) -> Result<(), InputsChanged> {
        match self {
            Self::Unchanged => Ok(()),
            Self::Changed { changes, remainder } => {
                Err(InputsChanged { changes, remainder })
            }
        }
    }
}

impl InputView {
    /// Re-observes everything tracked, in path order. A denial, containment
    /// violation, I/O failure or budget exhaustion is an error, never a
    /// `Change`. The pass has its own operation, byte and directory-entry
    /// counters, so it is not starved by the reads that were tracked.
    pub fn revalidate(
        &self,
        cx: &Cancellation,
    ) -> Result<Revalidation, InputError> {
        if cx.is_cancelled() {
            return Err(InputError::Cancelled);
        }
        let tracked = self.lock().evidence.clone();
        let meter = Mutex::new(Meter::default());
        let mut changes = Vec::new();
        let mut detected = 0u64;
        for (path, entry) in &tracked.paths {
            if cx.is_cancelled() {
                return Err(InputError::Cancelled);
            }
            let Some(kind) = self.revalidate_path(&meter, path, entry, cx)?
            else {
                continue;
            };
            detected += 1;
            if changes.len() < self.limits.max_reported_changes {
                changes.push(Change {
                    path: path.clone(),
                    kind,
                });
            }
        }
        if detected == 0 {
            return Ok(Revalidation::Unchanged);
        }
        let remainder = detected - changes.len() as u64;
        Ok(Revalidation::Changed { changes, remainder })
    }

    fn revalidate_path(
        &self,
        meter: &Mutex<Meter>,
        path: &WorkspacePath,
        entry: &PathEvidence,
        cx: &Cancellation,
    ) -> Result<Option<ChangeKind>, InputError> {
        // Two observations of this path already disagreed during the pass:
        // that is a change by itself, whatever the path looks like now.
        if entry.conflict {
            return Ok(Some(conflict_kind(entry)));
        }
        let Some(baseline) = baseline(entry) else {
            return Ok(None);
        };
        self.begin(meter, path, cx)?;
        let result = self.compare(meter, path, baseline, cx);
        match result {
            // The exact spelling is not what is on disk (D8). A tracked name
            // that existed is gone (`Deleted`); one tracked as absent now has a
            // differently spelled entry, which a fresh access would reject, so
            // the input world changed (`Created`). Either way the pass fails.
            Err(InputError::CaseMismatch { .. }) => {
                Ok(Some(if matches!(baseline, Observation::Missing) {
                    ChangeKind::Created
                } else {
                    ChangeKind::Deleted
                }))
            }
            other => other,
        }
    }

    fn compare(
        &self,
        meter: &Mutex<Meter>,
        path: &WorkspacePath,
        baseline: &Observation,
        cx: &Cancellation,
    ) -> Result<Option<ChangeKind>, InputError> {
        Ok(match baseline {
            Observation::Missing => {
                self.capture_kind(path)?.map(|_| ChangeKind::Created)
            }
            Observation::Present(kind) => match self.capture_kind(path)? {
                None => Some(ChangeKind::Deleted),
                Some(now) if now != *kind => Some(ChangeKind::KindChanged),
                Some(_) => None,
            },
            Observation::File { len, digest, .. } => {
                match self.capture_file(meter, path, cx, &mut || {})? {
                    FileOutcome::Missing => Some(ChangeKind::Deleted),
                    FileOutcome::NotFile(_) => Some(ChangeKind::KindChanged),
                    FileOutcome::File(now) => {
                        // D3: only the bytes matter; identity, mtime and an
                        // atomic replace with the same content are not changes.
                        (now.digest != *digest
                            || now.bytes.len() as u64 != *len)
                            .then_some(ChangeKind::Modified)
                    }
                }
            }
            Observation::Dir {
                membership_digest: before,
                ..
            } => match self.capture_dir(meter, path, cx)? {
                DirOutcome::Missing => Some(ChangeKind::Deleted),
                DirOutcome::NotDir(_) => Some(ChangeKind::KindChanged),
                DirOutcome::Dir(now) => (membership_digest(&now.entries)
                    != *before)
                    .then_some(ChangeKind::MembershipChanged),
            },
        })
    }
}

/// The strongest observation: content beats kind beats absence. A path that
/// is not in conflict has only compatible observations, so this loses nothing.
fn baseline(entry: &PathEvidence) -> Option<&Observation> {
    let find = |wanted: fn(&Observation) -> bool| {
        entry.observations.iter().find(|seen| wanted(seen))
    };
    find(|seen| matches!(seen, Observation::File { .. }))
        .or_else(|| find(|seen| matches!(seen, Observation::Dir { .. })))
        .or_else(|| find(|seen| matches!(seen, Observation::Present(_))))
        .or_else(|| entry.observations.first())
}

/// What the first pair of disagreeing observations says happened.
fn conflict_kind(entry: &PathEvidence) -> ChangeKind {
    let seen = &entry.observations;
    for (index, before) in seen.iter().enumerate() {
        for after in &seen[index + 1..] {
            if before.same_fact(after) || before.compatible(after) {
                continue;
            }
            return match (before.kind(), after.kind()) {
                (None, Some(_)) => ChangeKind::Created,
                (Some(_), None) => ChangeKind::Deleted,
                (Some(a), Some(b)) if a != b => ChangeKind::KindChanged,
                (Some(EntryKind::Dir), Some(_)) => {
                    ChangeKind::MembershipChanged
                }
                _ => ChangeKind::Modified,
            };
        }
    }
    ChangeKind::Modified
}
