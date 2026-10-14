//! Stub replacement for `vswhom-sys` (wired via `[patch.crates-io]` in the workspace Cargo.toml).
//!
//! The real crate compiles `ext/vswhom.cpp` with `cc`, which needs `cl.exe`. We build with the
//! no-admin toolchain (rust-lld + xwin + rc.exe from NuGet, see Docs/core/architecture.md §8),
//! so there is no MSVC compiler. `embed-resource` only uses vswhom as a *last-resort* lookup for
//! `rc.exe`, after the `RC` env var and the Windows Kits registry — both of which we provide.
//! Returning "nothing found" is therefore equivalent on every machine.
//!
//! ponytail: delete this stub (and the patch entry) once VS Build Tools are installed, if wanted.

#![allow(non_camel_case_types)]

/// Same layout as the upstream `Find_Result` (field order and types must match exactly:
/// `vswhom` reads `windows_sdk_version: c_int` and the paths as `*mut wchar_t` = `*mut u16`).
#[repr(C)]
pub struct Find_Result {
    /// Zero if no Windows SDK found.
    pub windows_sdk_version: i32,
    pub windows_sdk_root: *mut u16,
    pub windows_sdk_um_library_path: *mut u16,
    pub windows_sdk_ucrt_library_path: *mut u16,
    pub vs_exe_path: *mut u16,
    pub vs_library_path: *mut u16,
}

/// Always "not found".
pub unsafe extern "C" fn vswhom_find_visual_studio_and_windows_sdk() -> Find_Result {
    Find_Result {
        windows_sdk_version: 0,
        windows_sdk_root: std::ptr::null_mut(),
        windows_sdk_um_library_path: std::ptr::null_mut(),
        windows_sdk_ucrt_library_path: std::ptr::null_mut(),
        vs_exe_path: std::ptr::null_mut(),
        vs_library_path: std::ptr::null_mut(),
    }
}

/// Nothing was allocated, nothing to free.
pub unsafe extern "C" fn vswhom_free_resources(_result: *mut Find_Result) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_nothing_found() {
        let r = unsafe { vswhom_find_visual_studio_and_windows_sdk() };
        assert_eq!(r.windows_sdk_version, 0);
        assert!(r.vs_exe_path.is_null());
    }
}
