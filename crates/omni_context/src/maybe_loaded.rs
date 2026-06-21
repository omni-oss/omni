use system_traits::impls::RealSys;

use crate::{Context, ContextError, ContextSys, LoadedContext};

/// A context that is either already loaded or will be loaded on first use.
///
/// Pass a pre-loaded [`LoadedContext`] when the caller has already paid the
/// project-discovery cost; pass an unloaded [`Context`] when loading should
/// happen on demand. Either way, call [`into_loaded`] to obtain a
/// [`LoadedContext`] — the call is a zero-cost passthrough in the `Loaded`
/// state.
///
/// [`into_loaded`]: MaybeLoaded::into_loaded
#[derive(Clone, Debug)]
pub enum MaybeLoaded<TSys: ContextSys = RealSys> {
    /// The context has not yet been loaded.
    Unloaded(Context<TSys>),
    /// The context has already been loaded; [`into_loaded`] returns it as-is.
    ///
    /// [`into_loaded`]: MaybeLoaded::into_loaded
    Loaded(LoadedContext<TSys>),
}

impl<TSys: ContextSys> MaybeLoaded<TSys> {
    /// Returns a reference to the inner unloaded [`Context`].
    ///
    /// When the value is `Loaded`, returns the context that was used to
    /// construct the [`LoadedContext`].
    pub fn as_context(&self) -> &Context<TSys> {
        match self {
            Self::Unloaded(ctx) => ctx,
            Self::Loaded(loaded) => loaded.as_context(),
        }
    }

    /// Returns a reference to the inner [`LoadedContext`], or `None` if the
    /// context has not been loaded yet.
    pub fn try_as_loaded_context(&self) -> Option<&LoadedContext<TSys>> {
        match self {
            Self::Unloaded(_) => None,
            Self::Loaded(loaded) => Some(loaded),
        }
    }

    /// Returns a reference to the inner [`LoadedContext`].
    ///
    /// # Panics
    ///
    /// Panics if the context is in the [`Unloaded`] state. Call [`load`]
    /// first to ensure the context is loaded.
    ///
    /// [`Unloaded`]: MaybeLoaded::Unloaded
    /// [`load`]: MaybeLoaded::load
    pub fn as_loaded_context(&self) -> &LoadedContext<TSys> {
        match self {
            Self::Unloaded(_) => {
                panic!(
                    "called as_loaded_context on an unloaded context; call load() first"
                )
            }
            Self::Loaded(loaded) => loaded,
        }
    }

    pub async fn ensure_loaded(
        &mut self,
    ) -> Result<&LoadedContext<TSys>, ContextError> {
        self.load().await?;
        Ok(self.as_loaded_context())
    }

    /// Resolves to a [`LoadedContext`].
    ///
    /// If this value is `Loaded`, returns it directly (no I/O). Otherwise the
    /// context is loaded now via [`Context::into_loaded`].
    pub async fn into_loaded(
        self,
    ) -> Result<LoadedContext<TSys>, ContextError> {
        match self {
            Self::Unloaded(ctx) => ctx.into_loaded().await,
            Self::Loaded(loaded) => Ok(loaded),
        }
    }

    /// Loads the context in place if it is not already loaded.
    ///
    /// After this call returns `Ok(())`, [`as_loaded_context`] and
    /// [`try_as_loaded_context`] are guaranteed to return the loaded context.
    /// Calling this on an already-loaded value is a no-op.
    ///
    /// [`as_loaded_context`]: MaybeLoaded::as_loaded_context
    /// [`try_as_loaded_context`]: MaybeLoaded::try_as_loaded_context
    pub async fn load(&mut self) -> Result<(), ContextError> {
        if let Self::Unloaded(ctx) = self {
            let loaded = ctx.clone().into_loaded().await?;
            *self = Self::Loaded(loaded);
        }
        Ok(())
    }
}

impl<TSys: ContextSys> From<Context<TSys>> for MaybeLoaded<TSys> {
    fn from(ctx: Context<TSys>) -> Self {
        Self::Unloaded(ctx)
    }
}

impl<TSys: ContextSys> From<LoadedContext<TSys>> for MaybeLoaded<TSys> {
    fn from(loaded: LoadedContext<TSys>) -> Self {
        Self::Loaded(loaded)
    }
}
