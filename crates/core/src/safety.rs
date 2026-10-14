//! Path classification guard. Every path that comes from outside the core
//! (UI selection, MCP tool argument, preset file) goes through [`SafePath`]
//! before any destructive operation.

use std::path::{Component, Path, PathBuf};

/// Where a path lives, from the point of view of "may we delete here?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathClass {
    /// `C:\Windows\System32`, `WinSxS`, boot files: never touched by cleanup.
    SystemCritical,
    /// Rest of `C:\Windows` and `ProgramData`: only catalog entries.
    System,
    /// Current user's profile.
    UserProfile,
    /// Another user's profile: never touched.
    OtherUserProfile,
    /// Anything else on a fixed/removable drive.
    Other,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SafePathError {
    #[error("path must be absolute")]
    NotAbsolute,
    #[error("path contains '..' or '.' components")]
    Relative,
    #[error("path is inside a system-critical folder")]
    SystemCritical,
    #[error("path belongs to another user's profile")]
    OtherUser,
    #[error("UNC / network paths are not allowed")]
    Unc,
}

/// A validated absolute path plus its class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafePath {
    path: PathBuf,
    class: PathClass,
}

impl SafePath {
    /// Validate `raw` against the rules. `current_profile` is the running
    /// user's profile dir (`%USERPROFILE%`), `windows_dir` is `%SystemRoot%`.
    pub fn classify(
        raw: &Path,
        windows_dir: &Path,
        current_profile: &Path,
    ) -> Result<Self, SafePathError> {
        if !raw.is_absolute() {
            return Err(SafePathError::NotAbsolute);
        }
        if raw
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(SafePathError::Relative);
        }
        if let Some(Component::Prefix(p)) = raw.components().next()
            && matches!(
                p.kind(),
                std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..)
            )
        {
            return Err(SafePathError::Unc);
        }

        let lower = |p: &Path| p.to_string_lossy().to_ascii_lowercase();
        let raw_l = lower(raw);
        let win_l = lower(windows_dir);
        let prof_l = lower(current_profile);

        let critical = ["system32", "syswow64", "winsxs", "boot", "servicing"]
            .iter()
            .any(|sub| starts_with_dir(&raw_l, &format!("{win_l}\\{sub}")));
        if critical || raw_l == win_l {
            return Err(SafePathError::SystemCritical);
        }

        let class = if starts_with_dir(&raw_l, &win_l) {
            PathClass::System
        } else if starts_with_dir(&raw_l, &prof_l) {
            PathClass::UserProfile
        } else if let Some(users_root) = current_profile.parent()
            && starts_with_dir(&raw_l, &lower(users_root))
        {
            // Under C:\Users but not under the current profile.
            // `C:\Users\Public` is shared, treat as Other.
            let rest = &raw_l[lower(users_root).len()..];
            if rest.trim_start_matches('\\').starts_with("public") {
                PathClass::Other
            } else {
                return Err(SafePathError::OtherUser);
            }
        } else if raw_l.contains("\\programdata") {
            PathClass::System
        } else {
            PathClass::Other
        };

        Ok(Self {
            path: raw.to_path_buf(),
            class,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn class(&self) -> PathClass {
        self.class
    }
}

/// `child` is `dir` itself or lives below it (case-insensitive, already lowered).
fn starts_with_dir(child: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('\\');
    child == dir || child.starts_with(&format!("{dir}\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> PathBuf {
        PathBuf::from(r"C:\Windows")
    }
    fn me() -> PathBuf {
        PathBuf::from(r"C:\Users\Windows")
    }
    fn classify(p: &str) -> Result<SafePath, SafePathError> {
        SafePath::classify(Path::new(p), &win(), &me())
    }

    #[test]
    fn rejects_relative_and_dotdot() {
        assert_eq!(classify(r"Temp\x").unwrap_err(), SafePathError::NotAbsolute);
        assert_eq!(
            classify(r"C:\Users\Windows\..\Other").unwrap_err(),
            SafePathError::Relative
        );
    }

    #[test]
    fn rejects_system_critical_and_unc() {
        assert_eq!(
            classify(r"C:\Windows\System32\drivers").unwrap_err(),
            SafePathError::SystemCritical
        );
        assert_eq!(
            classify(r"C:\WINDOWS\WinSxS").unwrap_err(),
            SafePathError::SystemCritical
        );
        assert_eq!(classify(r"C:\Windows").unwrap_err(), SafePathError::SystemCritical);
        assert_eq!(
            classify(r"\\server\share\x").unwrap_err(),
            SafePathError::Unc
        );
    }

    #[test]
    fn classifies_profiles() {
        assert_eq!(
            classify(r"C:\Users\Windows\AppData\Local\Temp").unwrap().class(),
            PathClass::UserProfile
        );
        assert_eq!(
            classify(r"C:\Users\Other\AppData").unwrap_err(),
            SafePathError::OtherUser
        );
        assert_eq!(classify(r"C:\Users\Public\x").unwrap().class(), PathClass::Other);
        assert_eq!(classify(r"C:\Windows\Temp").unwrap().class(), PathClass::System);
        assert_eq!(classify(r"C:\ProgramData\NVIDIA").unwrap().class(), PathClass::System);
        assert_eq!(classify(r"D:\Games").unwrap().class(), PathClass::Other);
    }
}
