use crate::{ExecutionEventSubscriber, GeneratorEventSubscriber};

/// Combined subscriber trait for use in `omni_api`.
///
/// This is a supertrait of both [`ExecutionEventSubscriber`] and
/// [`GeneratorEventSubscriber`]. The blanket impl means any type implementing
/// both sub-traits automatically satisfies `OmniEventSubscriber` — no manual
/// impl required.
pub trait OmniEventSubscriber:
    ExecutionEventSubscriber + GeneratorEventSubscriber + Send + Sync
{
}

impl<T: ExecutionEventSubscriber + GeneratorEventSubscriber> OmniEventSubscriber for T {}
