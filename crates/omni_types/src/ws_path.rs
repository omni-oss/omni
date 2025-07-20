use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WsPath {
    path: PathBuf,
    is_ws_rooted: bool,
}

impl WsPath {
    pub fn new(path: impl Into<PathBuf>, is_ws_rooted: bool) -> Self {
        Self {
            path: path.into(),
            is_ws_rooted,
        }
    }

    pub fn is_ws_rooted(&self) -> bool {
        self.is_ws_rooted
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for WsPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.is_ws_rooted() {
            format!("workspace://{}", self.path.to_string_lossy())
                .serialize(serializer)
        } else {
            self.path.serialize(serializer)
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for WsPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.starts_with("workspace://") {
            Ok(Self::new(
                PathBuf::from(s.strip_prefix("workspace://").unwrap()),
                true,
            ))
        } else {
            Ok(Self::new(s, false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize() {
        let path = WsPath::new("/foo", true);
        assert_eq!(
            serde_json::to_string(&path).unwrap(),
            r#""workspace:///foo""#
        );

        let path = WsPath::new("/foo", false);
        assert_eq!(serde_json::to_string(&path).unwrap(), r#""/foo""#);
    }

    #[test]
    fn test_deserialize() {
        let path = WsPath::new("foo", true);
        assert_eq!(
            serde_json::from_str::<WsPath>(r#""workspace://foo""#).unwrap(),
            path
        );

        let path = WsPath::new("foo", false);
        assert_eq!(serde_json::from_str::<WsPath>(r#""foo""#).unwrap(), path);
    }
}
