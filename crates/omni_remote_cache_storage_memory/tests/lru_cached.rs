use std::num::NonZeroUsize;

use omni_remote_cache_storage_core::{
    decl_remote_cache_storage_backend_tests, decorators::LruCached,
};
use omni_remote_cache_storage_memory::InMemoryBackend;

decl_remote_cache_storage_backend_tests!(LruCached::new(
    InMemoryBackend::new("default"),
    NonZeroUsize::new(100).unwrap()
));
