//! Which directory entries are candidate scans (03_architecture section 4.3, step 1).
//!
//! ASD scan names, any case (DECISIONS 87-88): `.asd` (LabSpec 4, Trek-type handheld), a three-digit extension
//! (`.000`, the FieldSpec 3 numbering) and `.sco` (ViewSpec splice-corrected). Skipped: dotfiles, including macOS
//! AppleDouble `._name.asd` companions that Finder writes on non-HFS volumes and SMB shares; Office-style lock and
//! temp names (`~$x.asd`, `~x.asd`); anything whose final extension is not a scan extension (so `x.asd.tmp`,
//! `x.asd.part`, `x.asd~`, `.crdownload` never match). Hidden or system files (Windows attributes, macOS
//! `UF_HIDDEN`) are skipped at stat time ([`is_hidden_meta`]). A `.sco` whose source is in the same folder is the
//! same scan and is skipped where the folder is listed ([`is_shadowed_sco`]).

use std::ffi::OsStr;
use std::fs::Metadata;
use std::path::Path;

/// True if this file name could be a scan (see the module docs) and is not a hidden, AppleDouble, lock or temp
/// name. Directories are rejected separately (at stat time).
pub fn is_candidate_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        // Non-UTF-8 names: compare on the lossy form; such files are still scans if they end in a scan extension.
        return is_candidate_str(&name.to_string_lossy());
    };
    is_candidate_str(name)
}

/// `.asd`, `.sco` or exactly three digits, after a non-empty stem (mirrors `spyder_core::read::is_scan_name`).
fn has_scan_extension(lower: &str) -> bool {
    let Some((stem, ext)) = lower.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (ext == "asd"
            || ext == "sco"
            || (ext.len() == 3 && ext.bytes().all(|b| b.is_ascii_digit())))
}

fn is_candidate_str(name: &str) -> bool {
    if name.starts_with('.') || name.starts_with('~') {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    if !has_scan_extension(&lower) {
        return false;
    }
    // "x.tmp.asd"-style temp names written by some copy tools.
    let stem = &lower[..lower.rfind('.').unwrap_or(0)];
    !(stem.ends_with(".tmp") || stem.ends_with(".part"))
}

/// A ViewSpec `X.sco` whose source `X` (a scan name) is in the same folder: the same scan, splice-corrected. The
/// source (as measured) is scored and the `.sco` skipped, so a scan is never counted twice (DECISIONS 88; mirrors
/// `spyder_core::read::is_shadowed_sco`).
pub fn is_shadowed_sco(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.len() <= 4 || !name.to_ascii_lowercase().ends_with(".sco") {
        return false;
    }
    let source = &name[..name.len() - 4];
    is_candidate_str(source)
        && !source.to_ascii_lowercase().ends_with(".sco")
        && path.with_file_name(source).is_file()
}

/// Hidden or system files are never scans.
pub fn is_hidden_meta(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        meta.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        const UF_HIDDEN: u32 = 0x8000;
        meta.st_flags() & UF_HIDDEN != 0
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = meta;
        false
    }
}

/// Natural order for file names ("Spectrum2" before "Spectrum10"), case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.as_bytes(), b.as_bytes());
    loop {
        match (x.first(), y.first()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&c), Some(&d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let nx = x.iter().take_while(|c| c.is_ascii_digit()).count();
                let ny = y.iter().take_while(|c| c.is_ascii_digit()).count();
                let (dx, dy) = (&x[..nx], &y[..ny]);
                let tx = trim_zeros(dx);
                let ty = trim_zeros(dy);
                let o = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if o != Ordering::Equal {
                    return o;
                }
                x = &x[nx..];
                y = &y[ny..];
            }
            (Some(&c), Some(&d)) => {
                let o = c.to_ascii_lowercase().cmp(&d.to_ascii_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x = &x[1..];
                y = &y[1..];
            }
        }
    }
}

fn trim_zeros(d: &[u8]) -> &[u8] {
    let k = d.iter().take_while(|&&c| c == b'0').count();
    &d[k.min(d.len().saturating_sub(1))..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn ok(s: &str) -> bool {
        is_candidate_name(&OsString::from(s))
    }

    #[test]
    fn scan_extensions_any_case() {
        assert!(ok("Spectrum00001.asd"));
        assert!(ok("AB CD12a_1.ASD"));
        assert!(ok("X10001_1.Asd"));
        assert!(ok("spot.000"));
        assert!(ok("spot.029.sco"));
        assert!(ok("SPOT.007.SCO"));
        assert!(!ok("Spectrum00001.asd.tmp"));
        assert!(!ok("Spectrum00001.asd~"));
        assert!(!ok("Spectrum00001.asd.part"));
        assert!(!ok("Spectrum00001.txt"));
        assert!(!ok("spot.00"));
        assert!(!ok("spot.0001"));
        assert!(!ok(".asd"));
        assert!(!ok(".000"));
        assert!(!ok("notes"));
    }

    #[test]
    fn a_sco_beside_its_source_is_shadowed() {
        let dir = std::env::temp_dir().join(format!("spyder_watch_sco_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["a.000", "a.000.sco", "b.001.sco"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert!(is_shadowed_sco(&dir.join("a.000.sco")));
        assert!(!is_shadowed_sco(&dir.join("b.001.sco")));
        assert!(!is_shadowed_sco(&dir.join("a.000")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hidden_appledouble_lock_and_temp_names_are_skipped() {
        assert!(!ok("._Spectrum00001.asd"));
        assert!(!ok(".hidden.asd"));
        assert!(!ok("~$Spectrum00001.asd"));
        assert!(!ok("~Spectrum00001.asd"));
        assert!(!ok("Spectrum00001.tmp.asd"));
        assert!(!ok("spot.000.part.sco"));
    }

    #[test]
    fn natural_order() {
        let mut v = vec![
            "Spectrum10.asd",
            "Spectrum2.asd",
            "spectrum1.asd",
            "Spectrum002.asd",
        ];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            [
                "spectrum1.asd",
                "Spectrum002.asd",
                "Spectrum2.asd",
                "Spectrum10.asd"
            ]
        );
    }
}
