//! Is a folder on a network volume? Network folders are polled (03_architecture section 4.2; PLAN risk 9):
//! SMB/NFS change notifications are unreliable, and macOS FSEvents does not see writes made by another machine.
//!
//! * Windows: UNC paths (`\\server\share`, `\\?\UNC\...`) and mapped network drives (`GetDriveTypeW` =
//!   `DRIVE_REMOTE`), checked on the path as given and on its canonical form.
//! * macOS: `statfs` without `MNT_LOCAL`, or a network file-system type (smbfs, afpfs, nfs, webdav, ...).
//! * Linux: `statfs` magic numbers of NFS, SMB/CIFS, 9p (WSL drive mounts), AFS, Coda, NCP.
//!
//! Detection is cheap and best effort; the user can always force POLL mode per folder.

use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeKind {
    Local,
    Network,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub kind: VolumeKind,
    /// Plain words for the status line, e.g. "network drive Z:" or "local disk".
    pub detail: String,
}

impl VolumeInfo {
    fn local(detail: impl Into<String>) -> Self {
        VolumeInfo {
            kind: VolumeKind::Local,
            detail: detail.into(),
        }
    }
    fn network(detail: impl Into<String>) -> Self {
        VolumeInfo {
            kind: VolumeKind::Network,
            detail: detail.into(),
        }
    }
}

/// Detect the volume kind of `path` (which may not exist yet; then its nearest existing ancestor is used where
/// the platform call needs an existing path).
pub fn detect(path: &Path) -> VolumeInfo {
    let given = imp::detect(path);
    if given.kind == VolumeKind::Network {
        return given;
    }
    if let Ok(canon) = std::fs::canonicalize(path) {
        let c = imp::detect(&canon);
        if c.kind == VolumeKind::Network {
            return c;
        }
    }
    given
}

/// Pure part of the Windows rule (testable on every OS): `Some(network)` for UNC paths, `Some(local)` for
/// device paths without a drive, `None` when the drive letter must be asked of the OS.
pub fn classify_windows_path(s: &str) -> Result<VolumeInfo, char> {
    let s = s.replace('/', "\\");
    let upper = s.to_ascii_uppercase();
    if let Some(rest) = upper.strip_prefix(r"\\?\UNC\") {
        return Ok(VolumeInfo::network(format!(
            "network share \\\\{}",
            share_of(&s[s.len() - rest.len()..])
        )));
    }
    let body = if upper.starts_with(r"\\?\") || upper.starts_with(r"\\.\") {
        &s[4..]
    } else if let Some(rest) = s.strip_prefix(r"\\") {
        return Ok(VolumeInfo::network(format!(
            "network share \\\\{}",
            share_of(rest)
        )));
    } else {
        &s[..]
    };
    let mut chars = body.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) if d.is_ascii_alphabetic() => Err(d.to_ascii_uppercase()),
        _ => Ok(VolumeInfo::local("local disk")),
    }
}

fn share_of(rest: &str) -> String {
    rest.split('\\').take(2).collect::<Vec<_>>().join("\\")
}

#[cfg(windows)]
mod imp {
    use super::*;

    pub fn detect(path: &Path) -> VolumeInfo {
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        match classify_windows_path(&abs.to_string_lossy()) {
            Ok(v) => v,
            Err(drive) => {
                const DRIVE_REMOTE: u32 = 4;
                let root: Vec<u16> = format!("{drive}:\\")
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call; GetDriveTypeW only reads it.
                let t = unsafe {
                    windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr())
                };
                if t == DRIVE_REMOTE {
                    VolumeInfo::network(format!("network drive {drive}:"))
                } else {
                    VolumeInfo::local(format!("local drive {drive}:"))
                }
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod imp {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    fn statfs_of(path: &Path) -> Option<libc::statfs> {
        // Walk up to an existing ancestor (the folder may be missing while a share is disconnected).
        let mut p = Some(path);
        while let Some(cur) = p {
            if cur.exists() {
                let c = CString::new(cur.as_os_str().as_bytes()).ok()?;
                // SAFETY: `c` is a valid NUL-terminated path and `st` is a properly sized, writable statfs.
                let mut st: libc::statfs = unsafe { std::mem::zeroed() };
                let rc = unsafe { libc::statfs(c.as_ptr(), &mut st) };
                return (rc == 0).then_some(st);
            }
            p = cur.parent();
        }
        None
    }

    #[cfg(target_os = "macos")]
    pub fn detect(path: &Path) -> VolumeInfo {
        const NETWORK_FS: [&str; 7] = ["smbfs", "afpfs", "nfs", "webdav", "cifs", "ftp", "cddafs"];
        let Some(st) = statfs_of(path) else {
            return VolumeInfo::local("unknown volume (statfs failed)");
        };
        // SAFETY: f_fstypename is a NUL-terminated C string filled in by statfs.
        let fstype = unsafe { std::ffi::CStr::from_ptr(st.f_fstypename.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let local = (st.f_flags as u64) & (libc::MNT_LOCAL as u64) != 0;
        if !local || NETWORK_FS.contains(&fstype.as_str()) {
            VolumeInfo::network(format!("network volume ({fstype})"))
        } else {
            VolumeInfo::local(format!("local volume ({fstype})"))
        }
    }

    #[cfg(target_os = "linux")]
    pub fn detect(path: &Path) -> VolumeInfo {
        const NETWORK_MAGIC: [(u64, &str); 9] = [
            (0x6969, "nfs"),
            (0x517B, "smb"),
            (0xFF53_4D42, "cifs"),
            (0xFE53_4D42, "smb2"),
            (0x0102_1997, "9p"),
            (0x5346_414F, "afs"),
            (0x7375_7245, "coda"),
            (0x564C, "ncp"),
            (0x0BD0_0BD0, "lustre"),
        ];
        let Some(st) = statfs_of(path) else {
            return VolumeInfo::local("unknown volume (statfs failed)");
        };
        let t = (st.f_type as u64) & 0xFFFF_FFFF;
        match NETWORK_MAGIC.iter().find(|(m, _)| *m == t) {
            Some((_, name)) => VolumeInfo::network(format!("network volume ({name})")),
            None => VolumeInfo::local("local volume"),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    use super::*;
    pub fn detect(_path: &Path) -> VolumeInfo {
        VolumeInfo::local("local (not detected on this platform)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unc_paths_are_network() {
        for p in [
            r"\\labpc\LabSpec\2026",
            r"\\?\UNC\labpc\LabSpec\2026",
            "//labpc/LabSpec",
        ] {
            let v = classify_windows_path(p).unwrap();
            assert_eq!(v.kind, VolumeKind::Network, "{p}");
            assert!(v.detail.contains("labpc"), "{p}: {}", v.detail);
        }
    }

    #[test]
    fn drive_letters_go_to_the_os() {
        assert_eq!(classify_windows_path(r"C:\LabSpec"), Err('C'));
        assert_eq!(classify_windows_path(r"\\?\z:\x"), Err('Z'));
    }

    #[test]
    fn temp_dir_is_local() {
        assert_eq!(detect(&std::env::temp_dir()).kind, VolumeKind::Local);
    }
}
