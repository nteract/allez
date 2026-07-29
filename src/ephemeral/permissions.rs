//! Owner-only directory creation, anchored to `VerifiedRoot` (Unix +
//! Windows). On Windows, also verifies a directory this feature reuses
//! across process invocations (the root and its `envs`/`cache` children)
//! rather than trusting it unconditionally, mirroring the rigor `paths.rs`'s
//! `open_or_create_directory`/`verify_directory` pair already applies on
//! Unix via `fstat`'s owner/mode check.

#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

use super::{error::EphemeralEnvError, lifecycle::EnvironmentId, paths::VerifiedRoot};

#[cfg(unix)]
use rustix::fs::{Mode, mkdirat};

/// Creates one environment prefix underneath the verified root's anchored envs directory.
#[cfg(unix)]
pub(crate) fn create_environment_directory(
    root: &VerifiedRoot,
    id: EnvironmentId,
) -> Result<PathBuf, EphemeralEnvError> {
    let name = id.to_string();
    mkdirat(root.envs_dir(), &name, Mode::from_raw_mode(0o700))
        .map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    Ok(root.path().join("envs").join(name))
}

/// Creates one environment prefix with an owner-only ACL under the verified root.
#[cfg(windows)]
pub(crate) fn create_environment_directory(
    root: &VerifiedRoot,
    id: EnvironmentId,
) -> Result<PathBuf, EphemeralEnvError> {
    let location = root.path().join("envs").join(id.to_string());
    create_directory_with_owner_only_acl(&location)?;
    if let Err(error) = verify_owner_only_directory(&location) {
        // `create_directory_with_owner_only_acl` above already ran
        // (creating the directory, or tolerating an already-existing one)
        // by the time this verification step can fail, so a real
        // directory can now exist on disk even though this function is
        // about to return `Err`. Clean it up here so every caller of this
        // function -- in particular `orphan.rs`'s `publish_environment`,
        // which otherwise has no way to know a directory was actually
        // created -- can keep treating any error from this function as
        // "nothing was left behind," matching the Unix side's single
        // atomic `mkdirat` call, which never has this two-step gap.
        let _best_effort_cleanup = std::fs::remove_dir(&location);
        return Err(error);
    }
    Ok(location)
}

/// Creates `path` with an owner-only ACL applied atomically at creation
/// (never a separate, later `chmod`-equivalent call), tolerating the
/// directory already existing. Every caller re-verifies via
/// [`verify_owner_only_directory`] afterward regardless of which branch ran,
/// mirroring `paths.rs`'s `open_or_create_directory`+`verify_directory` pair
/// on Unix: a pre-existing directory is checked exactly as rigorously as one
/// this call just created.
#[cfg(windows)]
pub(crate) fn create_directory_with_owner_only_acl(path: &Path) -> Result<(), EphemeralEnvError> {
    use std::{ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, ptr};

    use windows_sys::Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LocalFree},
        Security::{
            ACL, Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            Authorization::SDDL_REVISION_1, MakeAbsoluteSD, PSECURITY_DESCRIPTOR,
            SECURITY_ATTRIBUTES, SetSecurityDescriptorOwner, TOKEN_USER,
        },
        Storage::FileSystem::CreateDirectoryW,
    };

    let current_user_sid = current_user_sid_buffer()?;
    // SAFETY: FFI category 9 (pointer cast). The helper returned a
    // pointer-aligned buffer containing a fully initialized `TOKEN_USER`.
    let token_user = unsafe { &*current_user_sid.as_ptr().cast::<TOKEN_USER>() };
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // "D:PAI(A;OICI;FA;;;OW)": a DACL, Protected (blocks inheriting the
    // parent's own ACL) and Auto-Inherited, granting Full Access to the
    // Owner only (`FILE_ALL_ACCESS`) -- the Windows equivalent of Unix's `0o700`.
    let sddl = std::ffi::OsStr::new("D:PAI(A;OICI;FA;;;OW)")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut descriptor_size = 0_u32;
    // SAFETY: FFI category 8. Both nul-terminated UTF-16 buffers outlive their calls,
    // `nLength` is initialized, every BOOL is checked, and the descriptor returned by
    // Windows is released with `LocalFree` after `CreateDirectoryW` completes.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            &mut descriptor_size,
        )
    };
    if converted == 0 {
        return Err(EphemeralEnvError::UnwritableLocation);
    }

    let result = (|| {
        let mut absolute_size = 0_u32;
        let mut dacl_size = 0_u32;
        let mut sacl_size = 0_u32;
        let mut owner_size = 0_u32;
        let mut primary_group_size = 0_u32;
        // `ConvertStringSecurityDescriptorToSecurityDescriptorW` returns a
        // self-relative descriptor, while `SetSecurityDescriptorOwner` requires
        // absolute format. This sizing call obtains every backing-buffer size.
        // SAFETY: FFI category 8. `descriptor` is live, each size output is
        // writable, and null output buffers request the documented sizes.
        let _ = unsafe {
            MakeAbsoluteSD(
                descriptor,
                ptr::null_mut(),
                &mut absolute_size,
                ptr::null_mut(),
                &mut dacl_size,
                ptr::null_mut(),
                &mut sacl_size,
                ptr::null_mut(),
                &mut owner_size,
                ptr::null_mut(),
                &mut primary_group_size,
            )
        };
        if absolute_size == 0 {
            return Err(EphemeralEnvError::UnwritableLocation);
        }

        let mut absolute = aligned_word_buffer(absolute_size)?;
        let mut dacl = aligned_word_buffer(dacl_size)?;
        let mut sacl = aligned_word_buffer(sacl_size)?;
        let mut owner = aligned_word_buffer(owner_size)?;
        let mut primary_group = aligned_word_buffer(primary_group_size)?;
        // SAFETY: FFI category 8. Every non-null output points to a live,
        // pointer-aligned buffer of at least its paired size, and `descriptor`
        // remains live as the self-relative source.
        let made_absolute = unsafe {
            MakeAbsoluteSD(
                descriptor,
                absolute.as_mut_ptr().cast(),
                &mut absolute_size,
                aligned_buffer_pointer::<ACL>(&mut dacl),
                &mut dacl_size,
                aligned_buffer_pointer::<ACL>(&mut sacl),
                &mut sacl_size,
                aligned_buffer_pointer::<c_void>(&mut owner),
                &mut owner_size,
                aligned_buffer_pointer::<c_void>(&mut primary_group),
                &mut primary_group_size,
            )
        };
        if made_absolute == 0 {
            return Err(EphemeralEnvError::UnwritableLocation);
        }
        let absolute_descriptor = absolute.as_mut_ptr().cast::<c_void>();
        // SAFETY: FFI category 8. `absolute_descriptor` points to the valid
        // absolute descriptor produced above. `token_user.User.Sid` points into
        // `current_user_sid`, which remains live through `CreateDirectoryW`.
        let owner_set =
            unsafe { SetSecurityDescriptorOwner(absolute_descriptor, token_user.User.Sid, 0) };
        if owner_set == 0 {
            return Err(EphemeralEnvError::UnwritableLocation);
        }

        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                .map_err(|_| EphemeralEnvError::UnwritableLocation)?,
            lpSecurityDescriptor: absolute_descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: FFI category 8. `wide_path`, `attributes`, every buffer
        // referenced by the absolute descriptor, and the explicit owner SID
        // remain live for this call.
        let created = unsafe { CreateDirectoryW(wide_path.as_ptr(), &attributes) };
        // SAFETY: FFI category 8. Queried immediately after the call above,
        // before another Windows API call can overwrite the last-error value.
        let last_error = unsafe { GetLastError() };
        if created != 0 || last_error == ERROR_ALREADY_EXISTS {
            Ok(())
        } else {
            Err(EphemeralEnvError::UnwritableLocation)
        }
    })();

    // SAFETY: FFI category 12. `descriptor` came from the documented allocating API
    // above and is freed exactly once, whether `CreateDirectoryW` succeeded or failed.
    let freed = unsafe { LocalFree(descriptor) };
    if !freed.is_null() {
        return Err(EphemeralEnvError::UnwritableLocation);
    }
    result
}

#[cfg(windows)]
fn aligned_word_buffer(byte_count: u32) -> Result<Vec<usize>, EphemeralEnvError> {
    let byte_count =
        usize::try_from(byte_count).map_err(|_| EphemeralEnvError::UnwritableLocation)?;
    Ok(vec![
        0_usize;
        byte_count.div_ceil(std::mem::size_of::<usize>())
    ])
}

#[cfg(windows)]
fn aligned_buffer_pointer<T>(buffer: &mut [usize]) -> *mut T {
    if buffer.is_empty() {
        std::ptr::null_mut()
    } else {
        buffer.as_mut_ptr().cast()
    }
}

/// Verifies that an existing directory is genuinely owner-only: not a
/// reparse point (a symlink or junction another local account could plant
/// to redirect this feature's writes), owned exclusively by the current
/// user, and carrying a DACL that grants access to no principal other
/// than that owner -- the Windows equivalent of Unix's `verify_directory`
/// (`st_uid` + exact `0o700` check, which likewise rejects any access
/// beyond the owner, not merely confirms the owner field itself).
#[cfg(windows)]
pub(crate) fn verify_owner_only_directory(path: &Path) -> Result<(), EphemeralEnvError> {
    use std::{os::windows::ffi::OsStrExt, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE, LocalFree},
        Security::{
            ACL,
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        },
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL,
        },
    };

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: FFI category 8. `wide_path` is a nul-terminated UTF-16 buffer that
    // outlives this call; the returned handle is checked below and closed on
    // every return path. `FILE_FLAG_OPEN_REPARSE_POINT` opens a reparse point
    // (symlink/junction) as itself rather than following it, so the attribute
    // check below can actually detect one.
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(EphemeralEnvError::UnwritableLocation);
    }

    let result = (|| {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: FFI category 8. `information` is a valid writable out-pointer;
        // `handle` was checked above and remains live for this call.
        let inspected = unsafe { GetFileInformationByHandle(handle, &mut information) } != 0;
        if !inspected
            || information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(EphemeralEnvError::UnwritableLocation);
        }

        let mut owner_sid: PSID = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: FFI category 8. `handle` is live and all outputs are writable;
        // on success, `descriptor` owns `owner_sid` and `dacl` and remains live
        // through their final use below.
        let queried = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner_sid,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if queried != 0 {
            return Err(EphemeralEnvError::UnwritableLocation);
        }
        let dacl_is_protected = !descriptor.is_null()
            // SAFETY: FFI category 8. `descriptor` was returned by the successful
            // call above and remains valid until it is freed below.
            && unsafe { security_descriptor_has_protected_dacl(descriptor) };
        let verification = if owner_sid.is_null() || dacl.is_null() || !dacl_is_protected {
            Err(EphemeralEnvError::UnwritableLocation)
        } else {
            // SAFETY: FFI category 8. `dacl` was returned by the successful call
            // above and remains valid until `descriptor` is freed below; `owner_sid`
            // likewise remains valid for this same span.
            match (current_user_owns_sid(owner_sid), unsafe {
                dacl_grants_only_owner(dacl, owner_sid)
            }) {
                (Ok(true), true) => Ok(()),
                (Ok(_), _) => Err(EphemeralEnvError::UnwritableLocation),
                (Err(error), _) => Err(error),
            }
        };
        // SAFETY: FFI category 12. `descriptor` came from the documented allocating
        // API above and is freed exactly once, after every read of `owner_sid`/`dacl`
        // (which point into it) above has already completed.
        let freed = unsafe { LocalFree(descriptor) };
        if !freed.is_null() {
            return Err(EphemeralEnvError::UnwritableLocation);
        }
        verification
    })();

    // SAFETY: FFI category 12. This function owns `handle` and closes it exactly
    // once, on every return path.
    let closed = unsafe { CloseHandle(handle) } != 0;
    if !closed {
        return Err(EphemeralEnvError::UnwritableLocation);
    }
    result
}

/// Returns whether a security descriptor's DACL is protected from inheritance.
///
/// # Safety
///
/// `descriptor` must point to a valid security descriptor for this call's duration.
#[cfg(windows)]
unsafe fn security_descriptor_has_protected_dacl(
    descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
) -> bool {
    use windows_sys::Win32::Security::{GetSecurityDescriptorControl, SE_DACL_PROTECTED};

    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: FFI category 8. `descriptor` is valid per the caller contract,
    // and `control` and `revision` are valid writable out-pointers.
    let read = unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
    read != 0 && control & SE_DACL_PROTECTED != 0
}

/// Returns whether every ACE in `dacl` is a plain allow entry naming
/// `owner_sid` or the well-known Owner Rights SID as its sole trustee. Windows
/// access checks apply an Owner Rights ACE (`S-1-3-4`) to the object's current
/// owner, so accepting that trustee matches the SDDL used at creation without
/// granting another principal access. A null/absent DACL, multiple ACEs, any
/// deny/inherited/audit ACE, or any unrelated trustee is rejected.
///
/// # Safety
///
/// `dacl` must be a valid, non-null pointer to an `ACL` returned by a
/// successful `GetSecurityInfo` call, valid for the duration of this call.
#[cfg(windows)]
unsafe fn dacl_grants_only_owner(
    dacl: *const windows_sys::Win32::Security::ACL,
    owner_sid: windows_sys::Win32::Security::PSID,
) -> bool {
    use windows_sys::Win32::{
        Security::{
            ACCESS_ALLOWED_ACE, ACE_HEADER, EqualSid, GetAce, INHERITED_ACE, IsWellKnownSid,
            WinCreatorOwnerRightsSid,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
    };

    // The well-known, stable NT ACE-type value for a plain "allow" entry
    // (`ACCESS_ALLOWED_ACE_TYPE`); hardcoded rather than adding a
    // dependency on `Win32_System_SystemServices` for one constant.
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

    // SAFETY: FFI category 8 (caller obligation, documented above). `dacl` is
    // valid and non-null per this function's own safety contract.
    let ace_count = unsafe { (*dacl).AceCount };
    if ace_count != 1 {
        return false;
    }
    for index in 0..u32::from(ace_count) {
        let mut ace: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: FFI category 8. `dacl` remains valid for this call (see above);
        // `ace` is a valid writable out-pointer, and the pointer it receives on
        // success is owned by `dacl` itself -- never separately freed.
        let got_ace = unsafe { GetAce(dacl, index, &mut ace) };
        if got_ace == 0 || ace.is_null() {
            return false;
        }
        // SAFETY: FFI category 9 (pointer cast). `ace` was returned by the
        // successful call above and is guaranteed to begin with a well-formed
        // `ACE_HEADER` for the duration of `dacl`'s own validity.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE
            || u32::from(header.AceFlags) & INHERITED_ACE != 0
        {
            return false;
        }
        // SAFETY: FFI category 9 (pointer cast). `header.AceType` confirmed this
        // ACE is an `ACCESS_ALLOWED_ACE` immediately above, so reinterpreting the
        // same pointer at that wider type is valid for `dacl`'s own validity span.
        // `SidStart` is the documented variable-length-struct idiom: the SID's own
        // bytes begin at that field's address, not at its (unused) `u32` value.
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        if allowed.Mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS {
            return false;
        }
        let sid = std::ptr::addr_of!(allowed.SidStart)
            .cast_mut()
            .cast::<std::ffi::c_void>();
        // SAFETY: FFI category 8. `sid` points into `ace`, valid per above; `owner_sid`
        // remains valid for this same call per this function's own safety contract.
        let names_owner = unsafe { EqualSid(sid, owner_sid) } != 0;
        // SAFETY: FFI category 8. `sid` points to the complete SID embedded in the
        // validated allow ACE and remains valid for the duration of this call.
        let names_owner_rights = unsafe { IsWellKnownSid(sid, WinCreatorOwnerRightsSid) } != 0;
        if !names_owner && !names_owner_rights {
            return false;
        }
    }
    true
}

/// Returns aligned storage containing the current process's `TOKEN_USER`.
#[cfg(windows)]
fn current_user_sid_buffer() -> Result<Vec<usize>, EphemeralEnvError> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TOKEN_QUERY, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = std::ptr::null_mut();
    // SAFETY: FFI category 8. `GetCurrentProcess` returns a pseudo-handle that
    // never needs closing; `token` is a valid writable out-pointer.
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return Err(EphemeralEnvError::UnwritableLocation);
    }

    let result = (|| {
        let mut needed = 0_u32;
        // SAFETY: FFI category 8. `token` is live for this call; a null buffer
        // with a zero length is the documented way to query the required size.
        let _ =
            unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
        if needed == 0 {
            return Err(EphemeralEnvError::UnwritableLocation);
        }
        let mut buffer = aligned_word_buffer(needed)?;
        let mut written = 0_u32;
        // SAFETY: FFI category 8. `buffer` is pointer-aligned and sized to at
        // least `needed` bytes, `written` is a valid writable out-pointer, and
        // `token` remains live for this call.
        let read = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut written,
            )
        };
        if read == 0 {
            return Err(EphemeralEnvError::UnwritableLocation);
        }
        Ok(buffer)
    })();

    // SAFETY: FFI category 12. This function owns `token` and closes it exactly
    // once, on every return path.
    let closed = unsafe { CloseHandle(token) } != 0;
    if !closed {
        return Err(EphemeralEnvError::UnwritableLocation);
    }
    result
}

/// Compares `sid` against the current process's own user SID.
#[cfg(windows)]
fn current_user_owns_sid(
    sid: windows_sys::Win32::Security::PSID,
) -> Result<bool, EphemeralEnvError> {
    use windows_sys::Win32::Security::{EqualSid, TOKEN_USER};

    let buffer = current_user_sid_buffer()?;
    // SAFETY: FFI category 9 (pointer cast). The helper returned a
    // pointer-aligned buffer containing a fully initialized `TOKEN_USER`.
    let token_user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    // SAFETY: FFI category 8. `sid` is valid for this call's duration, and
    // `token_user.User.Sid` points into `buffer`, which remains live here.
    Ok(unsafe { EqualSid(sid, token_user.User.Sid) } != 0)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn anchored_environment_directory_is_created_with_owner_only_mode() {
        use std::os::unix::fs::MetadataExt;

        let temp = tempfile::tempdir().unwrap();
        let root = super::super::paths::verified_root(&temp.path().join("root")).unwrap();
        let id = super::super::lifecycle::EnvironmentId::new();
        let location = super::create_environment_directory(&root, id).unwrap();

        assert_eq!(std::fs::metadata(location).unwrap().mode() & 0o777, 0o700);
    }

    #[cfg(windows)]
    struct LocalDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

    #[cfg(windows)]
    impl Drop for LocalDescriptor {
        fn drop(&mut self) {
            // SAFETY: FFI category 12. The test helper owns the descriptor returned
            // by `ConvertStringSecurityDescriptorToSecurityDescriptorW` and frees it
            // exactly once after every pointer borrowed from it has gone out of scope.
            let freed = unsafe { windows_sys::Win32::Foundation::LocalFree(self.0) };
            assert!(freed.is_null());
        }
    }

    #[cfg(windows)]
    fn descriptor_from_sddl(sddl: &str) -> LocalDescriptor {
        use std::{os::windows::ffi::OsStrExt, ptr};

        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };

        let sddl = std::ffi::OsStr::new(sddl)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut descriptor = ptr::null_mut();
        // SAFETY: FFI category 8. `sddl` is a nul-terminated UTF-16 buffer that
        // outlives the call, and `descriptor` is a valid writable out-pointer.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        assert_ne!(converted, 0);
        assert!(!descriptor.is_null());
        LocalDescriptor(descriptor)
    }

    #[cfg(windows)]
    fn dacl_is_owner_only(sddl: &str) -> bool {
        use std::ptr;

        use windows_sys::Win32::Security::{GetSecurityDescriptorDacl, GetSecurityDescriptorOwner};

        let descriptor = descriptor_from_sddl(sddl);
        let mut dacl_present = 0;
        let mut dacl = ptr::null_mut();
        let mut dacl_defaulted = 0;
        // SAFETY: FFI category 8. `descriptor` owns a valid security descriptor,
        // and all remaining arguments are valid writable out-pointers.
        let read_dacl = unsafe {
            GetSecurityDescriptorDacl(
                descriptor.0,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        };
        let mut owner = ptr::null_mut();
        let mut owner_defaulted = 0;
        // SAFETY: FFI category 8. `descriptor` remains valid, and both remaining
        // arguments are valid writable out-pointers.
        let read_owner =
            unsafe { GetSecurityDescriptorOwner(descriptor.0, &mut owner, &mut owner_defaulted) };
        assert_ne!(read_dacl, 0);
        assert_ne!(read_owner, 0);
        assert_ne!(dacl_present, 0);
        // SAFETY: FFI category 8. The successful descriptor accessors above
        // returned `dacl` and `owner`, which remain valid while `descriptor` lives.
        unsafe { super::dacl_grants_only_owner(dacl, owner) }
    }

    #[cfg(windows)]
    #[test]
    fn created_directory_owner_matches_current_user_sid() {
        use std::{os::windows::ffi::OsStrExt, ptr};

        use windows_sys::Win32::Security::{
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            EqualSid, OWNER_SECURITY_INFORMATION, TOKEN_USER,
        };

        let temp = tempfile::tempdir().unwrap();
        let location = temp.path().join("owned");
        super::create_directory_with_owner_only_acl(&location).unwrap();
        let wide_location = location
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut owner = ptr::null_mut();
        let mut descriptor = ptr::null_mut();

        // SAFETY: FFI category 8. `wide_location` is a nul-terminated UTF-16
        // buffer that outlives this call, and the requested outputs are valid
        // writable pointers.
        let queried = unsafe {
            GetNamedSecurityInfoW(
                wide_location.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(queried, 0);
        assert!(!descriptor.is_null());
        let _descriptor = LocalDescriptor(descriptor);
        assert!(!owner.is_null());

        let current_user_sid = super::current_user_sid_buffer().unwrap();
        // SAFETY: FFI category 9 (pointer cast). The helper returned a
        // pointer-aligned buffer containing a fully initialized `TOKEN_USER`.
        let token_user = unsafe { &*current_user_sid.as_ptr().cast::<TOKEN_USER>() };
        // SAFETY: FFI category 8. `owner` remains live while `_descriptor`
        // owns its allocation, and `token_user.User.Sid` points into the live
        // `current_user_sid` buffer.
        let owner_matches = unsafe { EqualSid(owner, token_user.User.Sid) } != 0;

        assert!(owner_matches);
    }

    #[cfg(windows)]
    #[test]
    fn owner_rights_ace_is_accepted_as_owner_only() {
        assert!(dacl_is_owner_only("O:SYD:PAI(A;OICI;FA;;;OW)"));
    }

    #[cfg(windows)]
    #[test]
    fn inherited_owner_rights_ace_is_rejected() {
        assert!(!dacl_is_owner_only("O:SYD:PAI(A;OICIID;FA;;;OW)"));
    }

    #[cfg(windows)]
    #[test]
    fn read_only_owner_rights_ace_is_rejected() {
        assert!(!dacl_is_owner_only("O:SYD:PAI(A;OICI;FR;;;OW)"));
    }

    #[cfg(windows)]
    #[test]
    fn protected_dacl_control_is_accepted() {
        let descriptor = descriptor_from_sddl("O:SYD:PAI(A;OICI;FA;;;OW)");

        // SAFETY: FFI category 8. `descriptor` owns a valid security descriptor
        // for the duration of this call.
        let protected = unsafe { super::security_descriptor_has_protected_dacl(descriptor.0) };

        assert!(protected);
    }

    #[cfg(windows)]
    #[test]
    fn unprotected_dacl_control_is_rejected() {
        let descriptor = descriptor_from_sddl("O:SYD:AI(A;OICI;FA;;;OW)");

        // SAFETY: FFI category 8. `descriptor` owns a valid security descriptor
        // for the duration of this call.
        let protected = unsafe { super::security_descriptor_has_protected_dacl(descriptor.0) };

        assert!(!protected);
    }
}
