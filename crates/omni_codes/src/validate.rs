use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::{CodeRegistration, registrations, valid_code};

/// Codes that were removed and must never be registered again, with the
/// release that removed them.
pub fn retired_codes() -> &'static [(&'static str, &'static str)] {
    RETIRED
}

const RETIRED: &[(&str, &str)] = &[];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    BadFormat { code: String },
    PrefixConflict { prefix: String, crates: Vec<String> },
    Duplicate { code: String },
    UndeclaredPrefix { code: String, crate_name: String },
    MissingRelated { code: String, related: String },
    Retired { code: String, removed_in: String },
    DeprecatedNotStable { code: String },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadFormat { code } => {
                write!(f, "`{code}` is not `prefix.snake_case`")
            }
            Self::PrefixConflict { prefix, crates } => write!(
                f,
                "prefix `{prefix}` is declared by more than one crate: {}",
                crates.join(", ")
            ),
            Self::Duplicate { code } => {
                write!(f, "`{code}` is registered more than once")
            }
            Self::UndeclaredPrefix { code, crate_name } => write!(
                f,
                "`{code}` uses a prefix not declared by `{crate_name}` (add `omni_codes::prefixes!`)"
            ),
            Self::MissingRelated { code, related } => {
                write!(f, "`{code}` relates to unknown code `{related}`")
            }
            Self::Retired { code, removed_in } => write!(
                f,
                "`{code}` was removed in {removed_in} and cannot be reused"
            ),
            Self::DeprecatedNotStable { code } => {
                write!(f, "`{code}` is deprecated but not stable")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// Validates everything submitted by linked crates.
pub fn validate() -> Result<(), Vec<RegistryError>> {
    let regs: Vec<&CodeRegistration> = registrations().collect();
    validate_set(&regs, RETIRED)
}

/// Validates an explicit set of registrations against a retired list.
pub fn validate_set(
    regs: &[&CodeRegistration],
    retired: &[(&str, &str)],
) -> Result<(), Vec<RegistryError>> {
    let mut errors = Vec::new();

    let mut prefixes: HashMap<&str, HashSet<&str>> = HashMap::new();
    for r in regs {
        prefixes
            .entry(r.crate_name)
            .or_default()
            .extend(r.prefixes.iter().copied());
    }

    let mut owners: HashMap<&str, Vec<&str>> = HashMap::new();
    for (krate, set) in &prefixes {
        for p in set {
            owners.entry(p).or_default().push(krate);
        }
    }
    let mut conflicts: Vec<_> =
        owners.into_iter().filter(|(_, v)| v.len() > 1).collect();
    conflicts.sort();
    for (prefix, mut crates) in conflicts {
        crates.sort();
        errors.push(RegistryError::PrefixConflict {
            prefix: prefix.into(),
            crates: crates.into_iter().map(String::from).collect(),
        });
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut reported: HashSet<&str> = HashSet::new();
    for r in regs {
        for c in r.codes {
            if !valid_code(c.code) {
                errors.push(RegistryError::BadFormat {
                    code: c.code.into(),
                });
            }
            if !seen.insert(c.code) && reported.insert(c.code) {
                errors.push(RegistryError::Duplicate {
                    code: c.code.into(),
                });
            }
            let prefix = c.code.split('.').next().unwrap_or("");
            let owned = prefixes
                .get(r.crate_name)
                .is_some_and(|p| p.contains(prefix));
            if !owned {
                errors.push(RegistryError::UndeclaredPrefix {
                    code: c.code.into(),
                    crate_name: r.crate_name.into(),
                });
            }
            if let Some((_, rel)) = retired.iter().find(|(k, _)| *k == c.code) {
                errors.push(RegistryError::Retired {
                    code: c.code.into(),
                    removed_in: (*rel).into(),
                });
            }
            if c.deprecated && !c.stable {
                errors.push(RegistryError::DeprecatedNotStable {
                    code: c.code.into(),
                });
            }
        }
    }

    for r in regs {
        for c in r.codes {
            for rel in c.related {
                if !seen.contains(rel) {
                    errors.push(RegistryError::MissingRelated {
                        code: c.code.into(),
                        related: (*rel).into(),
                    });
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Audience, CodeDetail, CodeInfo};

    const fn info(code: &'static str) -> CodeInfo {
        CodeInfo {
            code,
            summary: "s",
            explanation: "",
            related: &[],
            stable: false,
            deprecated: false,
            detail: CodeDetail::Error {
                audience: Audience::User,
            },
        }
    }

    static A: &[CodeInfo] = &[info("cache.a"), info("cache.b")];
    static DUP: &[CodeInfo] = &[info("cache.a")];
    static FOREIGN: &[CodeInfo] = &[info("other.x")];
    static REL: &[CodeInfo] = &[CodeInfo {
        related: &["cache.nope"],
        ..info("cache.r")
    }];
    static DEP: &[CodeInfo] = &[CodeInfo {
        deprecated: true,
        ..info("cache.d")
    }];

    fn reg(
        codes: &'static [CodeInfo],
        prefixes: &'static [&'static str],
    ) -> CodeRegistration {
        CodeRegistration {
            crate_name: "c",
            codes,
            prefixes,
        }
    }

    #[test]
    fn valid_set_passes() {
        let r = reg(A, &["cache"]);
        assert!(validate_set(&[&r], &[]).is_ok());
    }

    #[test]
    fn duplicates_across_registrations_fail() {
        let a = reg(A, &["cache"]);
        let b = reg(DUP, &[]);
        let e = validate_set(&[&a, &b], &[]).unwrap_err();
        assert!(e.contains(&RegistryError::Duplicate {
            code: "cache.a".into()
        }));
    }

    #[test]
    fn prefix_must_be_declared() {
        let a = reg(FOREIGN, &["cache"]);
        let e = validate_set(&[&a], &[]).unwrap_err();
        assert!(matches!(e[0], RegistryError::UndeclaredPrefix { .. }));
    }

    #[test]
    fn a_prefix_has_one_owner() {
        let a = reg(A, &["cache"]);
        let b = CodeRegistration {
            crate_name: "other",
            codes: &[],
            prefixes: &["cache"],
        };
        let e = validate_set(&[&a, &b], &[]).unwrap_err();
        assert!(matches!(e[0], RegistryError::PrefixConflict { .. }));
    }

    #[test]
    fn related_must_exist() {
        let a = reg(REL, &["cache"]);
        let e = validate_set(&[&a], &[]).unwrap_err();
        assert!(matches!(e[0], RegistryError::MissingRelated { .. }));
    }

    #[test]
    fn retired_codes_cannot_return() {
        let a = reg(A, &["cache"]);
        let e = validate_set(&[&a], &[("cache.a", "0.1.0")]).unwrap_err();
        assert!(matches!(e[0], RegistryError::Retired { .. }));
    }

    #[test]
    fn deprecated_must_be_stable() {
        let a = reg(DEP, &["cache"]);
        let e = validate_set(&[&a], &[]).unwrap_err();
        assert!(matches!(e[0], RegistryError::DeprecatedNotStable { .. }));
    }
}
