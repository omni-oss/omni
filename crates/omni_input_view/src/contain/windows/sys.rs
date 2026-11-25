//! The one unsafe call of the crate: `GetFinalPathNameByHandleW`, behind a
//! safe signature. `cap-primitives` keeps its own equivalent crate-private.

#![allow(unsafe_code)]

use std::{
    ffi::OsString,
    fs::File,
    io,
    os::windows::{ffi::OsStringExt, io::AsRawHandle},
    path::PathBuf,
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_DOS},
};

/// Size-query then fill, retried a few times if the path grows in between.
pub(super) fn final_path(file: &File) -> io::Result<PathBuf> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut buffer: Vec<u16> = vec![0; 512];
    for _ in 0..4 {
        let length = u32::try_from(buffer.len()).map_err(io::Error::other)?;
        // SAFETY: `handle` is a live handle owned by `file`, which is
        // borrowed for the whole call. `buffer` is a valid, writable region of
        // exactly `buffer.len()` UTF-16 units, and nothing is retained.
        let written = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                buffer.as_mut_ptr(),
                length,
                VOLUME_NAME_DOS,
            )
        } as usize;
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        if written < buffer.len() {
            return Ok(PathBuf::from(OsString::from_wide(&buffer[..written])));
        }
        // Too small: `written` is the required size including the NUL.
        buffer.resize(written, 0);
    }
    Err(io::Error::other("final path kept changing size"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_handle_has_a_final_path() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS
            .open(tmp.path())
            .unwrap();
        let found = final_path(&handle).unwrap();
        assert_eq!(found, std::fs::canonicalize(tmp.path()).unwrap());
    }

    #[test]
    fn a_path_over_max_path_needs_the_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let mut dir = std::fs::canonicalize(tmp.path()).unwrap();
        for _ in 0..7 {
            dir.push("d".repeat(100));
        }
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f.txt");
        std::fs::write(&file, b"x").unwrap();
        let handle = File::open(&file).unwrap();
        let found = final_path(&handle).unwrap();
        assert!(found.as_os_str().len() > 512);
        assert_eq!(found, file);
    }

    #[test]
    fn a_verbatim_root_prefix_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        let verbatim = std::fs::canonicalize(&file).unwrap();
        let handle = File::open(&verbatim).unwrap();
        assert_eq!(final_path(&handle).unwrap(), verbatim);
    }
}
