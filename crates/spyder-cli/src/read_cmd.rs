//! `spyder read`: per scan, acceptance, header fields, N2 per window and the acquisition checks B1-B8
//! (`--json`: schema `spyder-bone/read` v1; see README.md).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;
use spyder_core::n2::N2Set;
use spyder_core::qc::{self, CheckResult, Outcome};
use spyder_core::read::{self, ReadError};
use spyder_core::scan::{AsdHeader, Scan, ScanKind, Segment, Timestamps};
use std::fmt::Write as _;
use std::io::Write as _;

const SCHEMA: &str = "spyder-bone/read";
const SCHEMA_VERSION: u32 = 1;
const B6_REASON: &str = "B6 is per model and transfer: see `spyder predict` (noise_sd_pct); the B6 rule runs in the Phase 3 pipeline";

pub const USAGE: &str = "usage: spyder read <file.asd | folder> [--json] [--recursive]

  Reads ASD LabSpec 4 .asd files (as8, joins 1000/1800 nm) and prints, per scan,
  whether it is accepted, its header fields, N2 noise per window and the
  acquisition checks B1-B8.

  --json        one JSON document on stdout (schema spyder-bone/read v1)
  --recursive   include .asd files in sub-folders (links and junctions are not followed)
";

#[derive(Serialize)]
struct Acceptance {
    /// "accepted" | "not_scored" | "rejected"
    status: &'static str,
    /// Plain words.
    label: String,
    /// For rejected files: "truncated" | "not_asd" | "unsupported" | "invalid" | "io".
    error_kind: Option<&'static str>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct Summary {
    min_r_1000_2450: Option<f64>,
    max_r_1000_2450: Option<f64>,
    mean_r_1000_2450: Option<f64>,
}

#[derive(Serialize)]
struct N2Out {
    #[serde(rename = "2000_2100")]
    w2000_2100: Option<f64>,
    #[serde(rename = "1500_1600")]
    w1500_1600: Option<f64>,
    #[serde(rename = "1500_1550")]
    w1500_1550: Option<f64>,
    #[serde(rename = "2300_2400")]
    w2300_2400: Option<f64>,
}

/// The `--json` document (field order is the documented order).
#[derive(Serialize)]
struct Doc {
    schema: &'static str,
    schema_version: u32,
    spyder_core_version: &'static str,
    engine: String,
    files: Vec<FileReport>,
}

#[derive(Serialize)]
struct FileReport {
    path: String,
    file_name: String,
    acceptance: Acceptance,
    /// "sample" | "white_reference_save" | "dark_save"; null when rejected.
    kind: Option<ScanKind>,
    header: Option<AsdHeader>,
    timestamps: Option<Timestamps>,
    splices_nm: Option<Vec<f64>>,
    segments: Option<Vec<Segment>>,
    reflectance: Option<Summary>,
    n2: Option<N2Out>,
    checks: Vec<CheckResult>,
    worst_outcome: Outcome,
    warnings: Vec<String>,
}

fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

fn summary(scan: &Scan) -> Summary {
    let v: Vec<f64> = scan
        .wavelengths_nm
        .iter()
        .zip(&scan.reflectance)
        .filter(|(w, _)| (1000.0..=2450.0).contains(*w))
        .map(|(_, &r)| r)
        .collect();
    let min = v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    Summary {
        min_r_1000_2450: finite(min),
        max_r_1000_2450: finite(max),
        mean_r_1000_2450: finite(mean),
    }
}

fn report(path: &Path) -> FileReport {
    let read = read::read_file(path);
    let n2 = read
        .as_ref()
        .ok()
        .map(|s| N2Set::compute(&s.reflectance, &s.wavelengths_nm));
    let mut checks = qc::acquisition_checks(&read, n2.as_ref());
    // B6 sits after B5 in the B-order; it needs the Phase 0b gains
    let pos = checks
        .iter()
        .position(|c| c.id == "B6b")
        .unwrap_or(checks.len());
    let b6 = match &read {
        Ok(s) if s.kind.is_scored() => qc::b6_without_gains(B6_REASON),
        Ok(_) => qc::b6_without_gains("reference scan (not scored)"),
        Err(_) => qc::b6_without_gains("the file could not be read (B1)"),
    };
    checks.insert(pos, b6);
    let worst_outcome = qc::worst_outcome(&checks);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let path_s = path.to_string_lossy().to_string();
    match read {
        Ok(scan) => FileReport {
            path: path_s,
            file_name,
            acceptance: if scan.kind.is_scored() {
                Acceptance {
                    status: "accepted",
                    label: "accepted".into(),
                    error_kind: None,
                    reason: None,
                }
            } else {
                Acceptance {
                    status: "not_scored",
                    label: scan.kind.label().into(),
                    error_kind: None,
                    reason: None,
                }
            },
            kind: Some(scan.kind),
            reflectance: Some(summary(&scan)),
            n2: n2.map(|n| N2Out {
                w2000_2100: n.n2_2000_2100,
                w1500_1600: n.n2_1500_1600,
                w1500_1550: n.n2_1500_1550,
                w2300_2400: n.n2_2300_2400,
            }),
            header: Some(scan.header),
            timestamps: Some(scan.timestamps),
            splices_nm: Some(scan.splices_nm),
            segments: Some(scan.segments),
            checks,
            worst_outcome,
            warnings: scan.warnings,
        },
        Err(e) => FileReport {
            path: path_s,
            file_name,
            acceptance: Acceptance {
                status: "rejected",
                label: match &e {
                    ReadError::Unsupported { .. } => "unsupported".into(),
                    other => other.kind().replace('_', " "),
                },
                error_kind: Some(e.kind()),
                reason: Some(e.to_string()),
            },
            kind: None,
            header: None,
            timestamps: None,
            splices_nm: None,
            segments: None,
            reflectance: None,
            n2: None,
            checks,
            worst_outcome,
            warnings: Vec::new(),
        },
    }
}

/// The `.asd` files of a file or folder argument (recursive walks never follow links or junctions).
pub fn collect_inputs(path: &Path, recursive: bool) -> Result<Vec<PathBuf>, (u8, String)> {
    if path.is_dir() {
        let r = if recursive {
            read::list_asd_files_recursive(path)
        } else {
            read::list_asd_files(path)
        };
        r.map_err(|e| (1, format!("cannot list {}: {e}", path.display())))
    } else if path.exists() {
        Ok(vec![path.to_path_buf()])
    } else {
        Err((1, format!("no such file or folder: {}", path.display())))
    }
}

/// `writeln!` into a String (cannot fail).
#[macro_export]
macro_rules! w {
    ($o:expr, $($arg:tt)*) => {{
        let _ = writeln!($o, $($arg)*);
    }};
}

fn fmt_opt(x: Option<f64>, digits: usize) -> String {
    x.map(|v| format!("{v:.digits$}"))
        .unwrap_or_else(|| "n/a".into())
}

fn print_text(o: &mut String, r: &FileReport) {
    w!(o, "== {}", r.path);
    match r.acceptance.status {
        "rejected" => w!(
            o,
            "   REJECTED ({}): {}",
            r.acceptance.label,
            r.acceptance.reason.as_deref().unwrap_or("")
        ),
        _ => w!(o, "   {}", r.acceptance.label),
    }
    if let (Some(h), Some(t)) = (&r.header, &r.timestamps) {
        w!(o,
            "   serial {}  {}  program {:#04x}  joins {}/{} nm  IT {} ms  averages dark/ref/sample {}/{}/{}",
            h.serial,
            h.version,
            h.program_version,
            h.splice1_nm,
            h.splice2_nm,
            h.integration_time_ms,
            h.dark_averages,
            h.reference_averages,
            h.sample_averages
        );
        w!(
            o,
            "   SWIR gains {}/{}  offsets {}/{}  calibration series {}  instrument type {}",
            h.swir1_gain,
            h.swir2_gain,
            h.swir1_offset,
            h.swir2_offset,
            h.calibration_series,
            h.instrument_type
        );
        let off = t
            .utc_offset_minutes
            .map(|m| {
                format!(
                    "UTC{}{:02}:{:02}",
                    if m < 0 { '-' } else { '+' },
                    m.abs() / 60,
                    m.abs() % 60
                )
            })
            .unwrap_or_else(|| "UTC offset unknown".into());
        w!(
            o,
            "   sample {} (OLE {})  reference {} (OLE {})  {}  reference age {}",
            t.spectrum_local.as_deref().unwrap_or("?"),
            t.spectrum_ole,
            t.reference_local.as_deref().unwrap_or("?"),
            t.reference_ole,
            off,
            t.reference_age_s
                .map(|s| format!("{:.0} s", s))
                .unwrap_or_else(|| "n/a".into())
        );
    }
    if let Some(s) = &r.reflectance {
        w!(
            o,
            "   R(1000-2450): min {}  mean {}  max {}",
            fmt_opt(s.min_r_1000_2450, 4),
            fmt_opt(s.mean_r_1000_2450, 4),
            fmt_opt(s.max_r_1000_2450, 4)
        );
    }
    if let Some(n) = &r.n2 {
        w!(
            o,
            "   N2 (1e-5 A): 2000-2100 {}  1500-1600 {}  1500-1550 {}  2300-2400 {}",
            fmt_opt(n.w2000_2100, 3),
            fmt_opt(n.w1500_1600, 3),
            fmt_opt(n.w1500_1550, 3),
            fmt_opt(n.w2300_2400, 3)
        );
    }
    for c in &r.checks {
        let res = match c.outcome {
            Some(o) => format!("{o:?}").to_lowercase(),
            None => c.assessment.describe(),
        };
        let msg = c
            .message
            .as_deref()
            .map(|m| format!("  {m}"))
            .unwrap_or_default();
        w!(o, "   {:<4} {:<34} {}{}", c.id, c.name, res, msg);
    }
    for w in &r.warnings {
        w!(o, "   log: {w}");
    }
}

pub fn run(args: &[OsString]) -> Result<(), (u8, String)> {
    let mut json_out = false;
    let mut recursive = false;
    let mut target: Option<&OsString> = None;
    for a in args {
        match a.to_str().unwrap_or("") {
            "--json" => json_out = true,
            "--recursive" | "-r" => recursive = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            s if s.starts_with('-') => {
                return Err((
                    2,
                    format!(
                        "unknown option {s:?}

{USAGE}"
                    ),
                ))
            }
            _ => {
                if target.replace(a).is_some() {
                    return Err((
                        2,
                        format!(
                            "only one file or folder, please

{USAGE}"
                        ),
                    ));
                }
            }
        }
    }
    let Some(target) = target else {
        return Err((2, USAGE.to_string()));
    };
    let files = collect_inputs(Path::new(target), recursive)?;
    let reports: Vec<FileReport> = files.iter().map(|f| report(f)).collect();
    let mut out = String::new();
    if json_out {
        let doc = Doc {
            schema: SCHEMA,
            schema_version: SCHEMA_VERSION,
            spyder_core_version: env!("CARGO_PKG_VERSION"),
            engine: format!(
                "{}.{}",
                spyder_core::preprocess::ENGINE_VERSION.0,
                spyder_core::preprocess::ENGINE_VERSION.1
            ),
            files: reports,
        };
        out = serde_json::to_string_pretty(&doc).map_err(|e| (1, e.to_string()))?;
        out.push('\n');
    } else {
        for r in &reports {
            print_text(&mut out, r);
        }
        let accepted = reports
            .iter()
            .filter(|r| r.acceptance.status == "accepted")
            .count();
        let not_scored = reports
            .iter()
            .filter(|r| r.acceptance.status == "not_scored")
            .count();
        w!(
            out,
            "{} file(s): {} accepted, {} reference scan(s) not scored, {} rejected",
            reports.len(),
            accepted,
            not_scored,
            reports.len() - accepted - not_scored
        );
    }
    write_stdout(&out)
}

/// Write to stdout; a closed pipe (e.g. `| head`) is not an error worth a panic.
pub fn write_stdout(out: &str) -> Result<(), (u8, String)> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(out.as_bytes())
        .and_then(|_| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err((1, format!("cannot write output: {e}"))),
    }
}
