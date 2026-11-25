//! Limits are checked before the allocation they bound: an oversized file is
//! refused without allocating anything near its size.
#![cfg(any(target_os = "linux", windows))]
#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use omni_input_view::{
    AuthDecision, AuthError, Cancellation, InputAuthorizer, InputError,
    InputLimits, InputView, InputViewOptions, SymlinkPolicy,
};
use omni_path_utils::WorkspacePath;

struct Counting;

static LARGEST: AtomicUsize = AtomicUsize::new(0);

// SAFETY: delegates to the system allocator and only records sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        LARGEST.fetch_max(new_size, Ordering::SeqCst);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

struct AllowAll;

impl InputAuthorizer for AllowAll {
    fn authorize_read(
        &self,
        _: &WorkspacePath,
    ) -> Result<AuthDecision, AuthError> {
        Ok(AuthDecision::Allow)
    }
}

#[test]
fn an_oversized_file_is_refused_without_allocating_its_size() {
    const FILE_LEN: u64 = 16 * 1024 * 1024;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir(&root).unwrap();
    // Large enough to dwarf the 1 MiB limit and any allocator noise, small
    // enough not to matter if the filesystem is not sparse (NTFS).
    std::fs::File::create(root.join("huge"))
        .unwrap()
        .set_len(FILE_LEN)
        .unwrap();

    let view = InputView::open(
        &root,
        InputViewOptions {
            authorizer: Arc::new(AllowAll),
            limits: InputLimits {
                max_file_bytes: 1024 * 1024,
                ..InputLimits::default()
            },
            symlinks: SymlinkPolicy::Reject,
        },
    )
    .unwrap();

    LARGEST.store(0, Ordering::SeqCst);
    let result =
        view.read(&WorkspacePath::parse("huge").unwrap(), &Cancellation::new());
    let largest = LARGEST.load(Ordering::SeqCst);

    assert!(
        matches!(result, Err(InputError::TooLarge { len, limit }) if len == FILE_LEN && limit == 1024 * 1024)
    );
    assert!(
        largest < 1024 * 1024,
        "largest allocation was {largest} bytes"
    );
}
