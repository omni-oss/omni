/// A 32-byte content digest.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest([u8; 32]);

impl Digest {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0[..4] {
            write!(f, "{byte:02x}")?;
        }
        f.write_str("..")
    }
}

/// Incremental digest of the exact bytes returned. Local on purpose: it keeps
/// `omni_hasher` and its async and `system_traits` dependencies out of this
/// crate.
pub trait DigestSink {
    fn update(&mut self, bytes: &[u8]);
    fn finalize(self) -> Digest;
}

#[derive(Default)]
pub struct Blake3Digest(blake3::Hasher);

impl Blake3Digest {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DigestSink for Blake3Digest {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn finalize(self) -> Digest {
        Digest(*self.0.finalize().as_bytes())
    }
}
