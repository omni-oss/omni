use std::{fs::File, io, io::Write as _, path::Path};

pub(super) fn publish(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    #[cfg(windows)]
    let (file, existing) = windows::stage(parent, path)?;
    #[cfg(not(windows))]
    let file = tempfile::NamedTempFile::new_in(parent)?;
    let staged = finish_staging(file, |file| {
        file.write_all(contents)?;
        file.sync_all()
    })?;

    #[cfg(windows)]
    if existing {
        staged.persist(path)?;
    } else {
        // Do not overwrite a file another process just installed.
        staged.persist_noclobber(path)?;
    }
    #[cfg(not(windows))]
    staged.persist(path)?;
    Ok(())
}

fn finish_staging(
    mut file: tempfile::NamedTempFile,
    write_and_sync: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<tempfile::TempPath> {
    let result = write_and_sync(file.as_file_mut());
    // Close the file even on error: Windows cannot clean up the path while
    // the exclusive handle remains open.
    let staged = file.into_temp_path();
    result?;
    Ok(staged)
}

#[cfg(windows)]
mod windows {
    use std::{
        fs::File,
        io,
        mem::MaybeUninit,
        os::windows::{
            ffi::OsStrExt as _,
            io::{AsRawHandle as _, FromRawHandle as _},
        },
        path::Path,
        ptr::null_mut,
    };

    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GENERIC_WRITE,
            HANDLE, INVALID_HANDLE_VALUE, LocalFree,
        },
        Security::{
            ACE_HEADER, ACL,
            Authorization::{
                ConvertSidToStringSidW,
                ConvertStringSecurityDescriptorToSecurityDescriptorW,
                GetNamedSecurityInfoW, GetSecurityInfo, SDDL_REVISION_1,
                SE_FILE_OBJECT,
            },
            DACL_SECURITY_INFORMATION, GetAce, GetTokenInformation,
            INHERITED_ACE, InitializeSecurityDescriptor,
            LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED, SE_SACL_PROTECTED,
            SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
            SetSecurityDescriptorControl, SetSecurityDescriptorDacl,
            SetSecurityDescriptorOwner, SetSecurityDescriptorSacl, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::{CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    /// A protected DACL for a new store: the creating account and Local
    /// System only, with no inherited ACEs. Use the token's user SID rather
    /// than `OW`: an elevated administrator's default owner can be the
    /// Administrators group, which an unelevated run of the same account
    /// cannot use.
    fn private_dacl() -> io::Result<String> {
        let mut token: HANDLE = null_mut();
        // SAFETY: The pseudo-handle is valid and `token` is writable.
        if unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut needed = 0;
            // SAFETY: A null buffer asks Win32 for the required size.
            unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    null_mut(),
                    0,
                    &mut needed,
                )
            };
            if needed == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer =
                vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
            // SAFETY: The aligned buffer has at least `needed` bytes.
            if unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: Success wrote a TOKEN_USER whose SID lives in `buffer`.
            let sid =
                unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
            let mut text = null_mut();
            // SAFETY: `sid` is valid; `text` receives a LocalAlloc string.
            if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut len = 0;
            // SAFETY: Win32 returned a NUL-terminated UTF-16 string.
            while unsafe { *text.add(len) } != 0 {
                len += 1;
            }
            // SAFETY: The slice covers exactly the returned string.
            let sid = String::from_utf16(unsafe {
                std::slice::from_raw_parts(text, len)
            });
            // SAFETY: `text` was allocated by the API with LocalAlloc.
            unsafe { LocalFree(text.cast()) };
            let sid = sid.map_err(io::Error::other)?;
            Ok(format!("D:P(A;;FA;;;{sid})(A;;FA;;;SY)"))
        })();
        // SAFETY: `token` came from a successful OpenProcessToken call.
        unsafe { CloseHandle(token) };
        result
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain([0]).collect()
    }

    struct Descriptor(PSECURITY_DESCRIPTOR);

    impl Descriptor {
        fn from_sddl(sddl: &str) -> io::Result<Self> {
            let sddl = sddl.encode_utf16().chain([0]).collect::<Vec<_>>();
            let mut descriptor = null_mut();
            // SAFETY: The input is NUL-terminated and the output receives a
            // descriptor allocated by the Windows local allocator.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(descriptor))
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: The descriptor was allocated by a documented Win32
            // API requiring LocalFree; this wrapper owns it exactly once.
            unsafe { LocalFree(self.0) };
        }
    }

    pub(super) fn stage(
        parent: &Path,
        destination: &Path,
    ) -> io::Result<(tempfile::NamedTempFile, bool)> {
        let destination = wide(destination);
        let (mut owner, mut dacl, mut label, mut descriptor) =
            (null_mut(), null_mut(), null_mut(), null_mut());
        // SAFETY: The NUL-terminated path and writable outputs are valid.
        // On success the returned owner and DACL point into `descriptor`.
        let status = unsafe {
            GetNamedSecurityInfoW(
                destination.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | LABEL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                &mut label,
                &mut descriptor,
            )
        };
        let builder = tempfile::Builder::new();
        if status == ERROR_FILE_NOT_FOUND {
            let private = Descriptor::from_sddl(&private_dacl()?)?;
            let staged =
                builder.make_in(parent, |path| create_new(path, private.0))?;
            return Ok((staged, false));
        }
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let _original = Descriptor(descriptor);
        if owner.is_null() || dacl.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "existing credential file has no owner or discretionary ACL",
            ));
        }
        let mut copied = MaybeUninit::<SECURITY_DESCRIPTOR>::uninit();
        let copied = copied.as_mut_ptr().cast();
        // SAFETY: `label` is null or backed by the live original descriptor.
        let expected = unsafe { acl_entries(label) }?;
        let protection = SE_DACL_PROTECTED
            | if expected.is_empty() {
                0
            } else {
                SE_SACL_PROTECTED
            };
        const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
        // SAFETY: `copied` points to aligned storage initialized by the
        // first call. The owner and ACLs remain valid through `make_in`.
        // Protect the copied DACL so creation cannot add wider parent ACEs.
        unsafe {
            if InitializeSecurityDescriptor(
                copied,
                SECURITY_DESCRIPTOR_REVISION,
            ) == 0
                || SetSecurityDescriptorOwner(copied, owner, 0) == 0
                || SetSecurityDescriptorDacl(copied, 1, dacl, 0) == 0
                || (!expected.is_empty()
                    && SetSecurityDescriptorSacl(copied, 1, label, 0) == 0)
                || SetSecurityDescriptorControl(copied, protection, protection)
                    == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        // The same-directory rename keeps the staged file's owner and ACL.
        let staged =
            builder.make_in(parent, |path| create_new(path, copied))?;
        // An absent label is also meaningful (implicit medium integrity).
        // Never replace it with an explicit label inherited from the parent.
        if !file_label(staged.as_file()).is_ok_and(|actual| actual == expected)
        {
            drop(staged.into_temp_path());
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "could not preserve the credential file's mandatory integrity label",
            ));
        }
        Ok((staged, true))
    }

    fn file_label(file: &File) -> io::Result<Vec<Vec<u8>>> {
        let (mut label, mut descriptor) = (null_mut(), null_mut());
        // SAFETY: The handle and outputs are valid. The returned label
        // pointer refers to the descriptor taken into ownership below.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut label,
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let _descriptor = Descriptor(descriptor);
        // SAFETY: `label` is null or refers into the live descriptor.
        unsafe { acl_entries(label) }
    }

    // SAFETY: `acl` must be null or a live ACL returned by Win32.
    unsafe fn acl_entries(acl: *const ACL) -> io::Result<Vec<Vec<u8>>> {
        if acl.is_null() {
            return Ok(Vec::new());
        }
        // SAFETY: The caller keeps the ACL alive.
        let count = unsafe { (*acl).AceCount };
        (0..u32::from(count))
            .map(|index| {
                let mut entry = null_mut();
                // SAFETY: The ACL and output pointer are valid.
                if unsafe { GetAce(acl, index, &mut entry) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: GetAce returned a live ACE with its size in the header.
                let mut bytes = unsafe {
                    let size = (*entry.cast::<ACE_HEADER>()).AceSize;
                    std::slice::from_raw_parts(
                        entry.cast::<u8>(),
                        usize::from(size),
                    )
                    .to_vec()
                };
                // Inheritance provenance may change when the label is
                // copied explicitly, but its access mask and SID must not.
                bytes[1] &= !(INHERITED_ACE as u8);
                Ok(bytes)
            })
            .collect()
    }

    fn create_new(
        path: &Path,
        descriptor: PSECURITY_DESCRIPTOR,
    ) -> io::Result<File> {
        let path = wide(path);
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: The NUL-terminated path and security descriptor live until
        // the call returns. CREATE_NEW cannot open a pre-existing file.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE,
                0,
                &security,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: Successful CreateFileW returned an owned file handle.
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Write as _;
        use windows_sys::Win32::Security::{
            Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW,
            PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
        };

        fn dacl(path: &Path) -> String {
            let path = wide(path);
            let mut descriptor = null_mut();
            // SAFETY: The terminating NUL and writable output are valid.
            // The descriptor takes ownership of the returned allocation.
            let status = unsafe {
                GetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    &mut descriptor,
                )
            };
            assert_eq!(status, ERROR_SUCCESS);
            let descriptor = Descriptor(descriptor);
            let (mut text, mut length) = (null_mut(), 0);
            // SAFETY: The descriptor and both output pointers are valid.
            assert_ne!(
                unsafe {
                    ConvertSecurityDescriptorToStringSecurityDescriptorW(
                        descriptor.0,
                        SDDL_REVISION_1,
                        DACL_SECURITY_INFORMATION,
                        &mut text,
                        &mut length,
                    )
                },
                0
            );
            // SAFETY: Win32 allocated `length` UTF-16 units, including the
            // final NUL. The owned string is copied before freeing them.
            let text_slice = unsafe {
                std::slice::from_raw_parts(text, length as usize - 1)
            };
            let result = String::from_utf16(text_slice).unwrap();
            // SAFETY: Win32 allocated the string with the local allocator.
            unsafe { LocalFree(text.cast()) };
            result
        }

        #[test]
        fn staging_and_replacement_do_not_inherit_broader_access() {
            let parent = tempfile::tempdir().unwrap();
            let broad = Descriptor::from_sddl(
                "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)(A;OICI;FA;;;SY)",
            )
            .unwrap();
            let parent_wide = wide(parent.path());
            // SAFETY: The live descriptor and NUL-terminated path are valid.
            assert_ne!(
                unsafe {
                    SetFileSecurityW(
                        parent_wide.as_ptr(),
                        DACL_SECURITY_INFORMATION
                            | PROTECTED_DACL_SECURITY_INFORMATION,
                        broad.0,
                    )
                },
                0
            );
            assert!(dacl(parent.path()).contains(";;;WD)"));

            let new_path = parent.path().join("new-key");
            let (mut staged, existing) =
                stage(parent.path(), &new_path).unwrap();
            assert!(!existing);
            // A new store must name the creating account, not a group that
            // an unelevated run of the same account cannot use.
            let sid = private_dacl().unwrap();
            let sid = sid
                .strip_prefix("D:P(A;;FA;;;")
                .and_then(|rest| rest.strip_suffix(")(A;;FA;;;SY)"))
                .expect("DACL names the creating user's SID");
            assert!(
                sid.starts_with("S-1-5-21-") || sid.starts_with("S-1-12-1-")
            );
            // Close the exclusive handle before reading security via path.
            staged.write_all(b"replacement").unwrap();
            staged.as_file().sync_all().unwrap();
            let staged = staged.into_temp_path();
            let staged_acl = dacl(&staged);
            assert!(staged_acl.starts_with("D:P"));
            assert!(!staged_acl.contains(";;;WD)"));
            // The staged file itself, not just the requested descriptor,
            // must grant the creating account.
            assert!(
                staged_acl.contains(&format!(";;;{sid})")),
                "staged DACL {staged_acl} lacks the creating user {sid}"
            );
            let existing = parent.path().join("keystore");
            let existing_acl = Descriptor::from_sddl(
                "D:P(A;;FA;;;OW)(A;;FR;;;BA)(A;;FA;;;SY)S:P(ML;;NRNW;;;ME)",
            )
            .unwrap();
            create_new(&existing, existing_acl.0)
                .unwrap()
                .write_all(b"original")
                .unwrap();
            let before = dacl(&existing);
            let label = file_label(&File::open(&existing).unwrap()).unwrap();
            assert!(
                !label.is_empty(),
                "fixture needs an explicit integrity label"
            );
            assert_ne!(before, staged_acl);
            let (replacement, replaces_existing) =
                stage(parent.path(), &existing).unwrap();
            assert!(replaces_existing);
            assert_eq!(file_label(replacement.as_file()).unwrap(), label);
            let replacement = replacement.into_temp_path();
            assert_eq!(dacl(&replacement), before);
            super::super::publish(&existing, b"replacement").unwrap();
            assert_eq!(dacl(&existing), before);
            assert_eq!(
                file_label(&File::open(&existing).unwrap()).unwrap(),
                label
            );
            assert_eq!(std::fs::read(&existing).unwrap(), b"replacement");
        }

        #[test]
        fn failed_write_or_sync_removes_the_exclusively_open_snapshot() {
            for data in [b"partial snapshot".as_slice(), b"complete snapshot"] {
                let parent = tempfile::tempdir().unwrap();
                let destination = parent.path().join("keystore");
                let (staged, _) = stage(parent.path(), &destination).unwrap();
                let result = super::super::finish_staging(staged, |file| {
                    file.write_all(data)?;
                    Err(io::Error::other("injected write or sync failure"))
                });
                assert!(result.is_err());
                assert_eq!(
                    std::fs::read_dir(parent.path()).unwrap().count(),
                    0
                );
            }
        }

        #[test]
        fn unlabeled_store_cannot_be_replaced_with_an_inherited_low_label() {
            let parent = tempfile::tempdir().unwrap();
            let low_parent =
                Descriptor::from_sddl("S:(ML;OICI;NW;;;LW)").unwrap();
            let parent_wide = wide(parent.path());
            // SAFETY: The NUL-terminated path and descriptor live through the call.
            assert_ne!(
                unsafe {
                    SetFileSecurityW(
                        parent_wide.as_ptr(),
                        LABEL_SECURITY_INFORMATION,
                        low_parent.0,
                    )
                },
                0,
            );
            let plain =
                Descriptor::from_sddl(&private_dacl().unwrap()).unwrap();
            let probe = parent.path().join("inheritance-probe");
            create_new(&probe, plain.0).unwrap();
            assert!(
                !file_label(&File::open(&probe).unwrap()).unwrap().is_empty()
            );
            std::fs::remove_file(probe).unwrap();

            let destination = parent.path().join("keystore");
            let unlabeled =
                Descriptor::from_sddl("D:P(A;;FA;;;OW)(A;;FA;;;SY)S:P")
                    .unwrap();
            create_new(&destination, unlabeled.0)
                .unwrap()
                .write_all(b"existing")
                .unwrap();
            assert!(
                file_label(&File::open(&destination).unwrap())
                    .unwrap()
                    .is_empty()
            );

            let err = super::super::publish(&destination, b"replacement")
                .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
            assert!(err.to_string().contains("integrity label"));
            assert_eq!(std::fs::read(&destination).unwrap(), b"existing");
            assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
        }
    }
}
