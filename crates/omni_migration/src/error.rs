#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(String),
    #[error("missing version")]
    MissingVersion,
    #[error("malformed version")]
    MalformedVersion,
    #[error("unknown but well-formed version tag")]
    UnknownTag,
    #[error("unknown version {0:?}")]
    UnknownVersion(crate::Version),
    #[error("missing chain name")]
    MissingChain,
    #[error("malformed chain name")]
    MalformedChain,
    #[error("store belongs to a different chain")]
    ChainMismatch,
    #[error("catalog or chain differs from the plan")]
    StaleCatalog,
    #[error("store changed since the plan")]
    StalePlan,
    #[error("invalid chain")]
    InvalidChain,
    #[error("step not implemented")]
    MissingStep,
    #[error("no downgrade implementation on the reverse path")]
    NoDowngradePath,
    #[error("requested direction does not match the target")]
    WrongDirection,
    #[error("non-atomic step not allowed")]
    NonAtomic,
    #[error("derivable loss not allowed")]
    DerivableLoss,
    #[error("irreversible loss not allowed")]
    IrreversibleLoss,
    #[error("backend cannot honor the step")]
    UnsupportedBackend,
    #[error("store needs recovery")]
    Dirty,
    #[error("cancelled")]
    Cancelled,
    #[error("unexpected version during step")]
    UnexpectedVersion,
    #[error("downgrade would drop meaningful notes")]
    MeaningfulNotes,
    #[error("downgrade would drop meaningful tags")]
    MeaningfulTags,
    #[error("malformed document: {0}")]
    MalformedDocument(String),
    #[error("malformed intent")]
    MalformedIntent,
    #[error("inconsistent layout: {0}")]
    Inconsistent(String),
    #[error("recovery evidence does not match any known state")]
    RecoveryBlocked,
    #[error("another process holds the migration guard")]
    GuardBusy,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(format!("{:?}: {e}", e.kind()))
    }
}
