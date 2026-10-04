//! Plug-in discovery, loading and precedence (PLAN section 4; 03 section 5.2).
//!
//! * **Locations:** the bundled folder (read-only, shipped with the app) and the user folder; both are scanned
//!   recursively for `*.json` and validated identically. Hidden files, macOS `._*` companions and symlinked or
//!   junction directories (cycle safety) are skipped. Files without a known `format` are ignored (e.g. schemas).
//! * **Loading:** strict JSON, sidecars, schema-level validation per kind, then the goldens run (models,
//!   consensus files, transfers, bands, noise gains). Engine-check and profile goldens are checked for content
//!   (>= 3 cases) and run in Phase 3. A failing file is disabled with a plain reason.
//! * **Identity:** a file is identified by the SHA-256 of its bytes as read from disk (never normalised). The same
//!   id and version with different contents: both rejected. The same bytes twice: the first copy is used.
//! * **Cross-file:** consensus components must match a loaded model by id, version and file SHA-256; transfer
//!   keys in `bands.json` / `noise_gains.json` must match a transfer file in the same folder; engine checks must
//!   name bands of the loaded band table; profiles must name loaded models; catalog entries must match.
//! * **Precedence:** per (format, id) the highest `active` version across locations wins. A pin beats that (and
//!   may select a non-active, non-withdrawn version); a pinned version that is withdrawn, failed or absent falls
//!   back to the highest passing active version with a visible note. Nothing in the user folder is ever pruned.
//! * **Startup error:** a failing bundled `active` file (or a failing bundled catalog, or no usable verdict model)
//!   puts the app in the startup-error state ("no verdict model available"), listed in `startup_errors`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::checks::{parse_engine_check, EngineCheck};
use super::golden::{
    load_golden_spectra, parse_golden_spectra, run_model_goldens, run_transfer_goldens, GoldenRun,
    GoldenSpectra,
};
use super::json::{package_path, read_json_file, resolve_sidecars, Node};
use super::profiles::{parse_profile, AnalysisProfile};
use super::refset::{parse_reference_set, ReferenceSet};
use super::tables::{
    parse_bands, parse_instruments, parse_noise_gains, run_table_goldens, Bands, Instruments,
    NoiseGains, TableRef, NO_TRANSFER,
};
use super::{
    is_sha256_hex, ErrorKind, PluginError, PluginStatus, Version, BANDS_FORMAT, CATALOG_FORMAT,
    ENGINE_CHECK_FORMAT, GOLDEN_SPECTRA_FORMAT, INSTRUMENT_FORMAT, MODEL_FORMAT,
    NOISE_GAINS_FORMAT, PROFILE_FORMAT, REFERENCE_SET_FORMAT, TRANSFER_FORMAT,
};
use crate::model::{parse_model, Body, Model};
use crate::transfer::{parse_transfer, Transfer};

/// Where a plug-in came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Bundled,
    User,
}

/// A folder to scan.
#[derive(Debug, Clone)]
pub struct Location {
    pub dir: PathBuf,
    pub origin: Origin,
}

/// Pinned versions per plug-in id (settings).
pub type Pins = BTreeMap<String, Version>;

/// A parsed plug-in.
#[derive(Debug, Clone)]
pub enum Item {
    Model(Box<Model>),
    Transfer(Box<Transfer>),
    Bands(Box<Bands>),
    NoiseGains(Box<NoiseGains>),
    Instruments(Box<Instruments>),
    EngineCheck(Box<EngineCheck>),
    Profile(Box<AnalysisProfile>),
    ReferenceSet(Box<ReferenceSet>),
    GoldenSpectra { id: String, spectra: usize },
    Catalog { entries: Vec<(String, String)> },
}

/// Load state of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Valid; goldens (if any) passed.
    Loaded,
    /// Rejected with a reason (see `error`).
    Disabled,
    /// Not a plug-in, or a byte-identical duplicate.
    Ignored,
}

/// One file found in a location.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub origin: Origin,
    pub sha256: String,
    pub bytes: u64,
    pub format: Option<String>,
    pub id: Option<String>,
    pub version: Option<Version>,
    pub status: Option<PluginStatus>,
    pub state: State,
    pub error: Option<PluginError>,
    pub golden: Option<GoldenRun>,
    /// Golden cases checked for content but run by a later engine phase (engine checks, profiles).
    pub golden_deferred: Option<usize>,
    pub notes: Vec<String>,
    pub item: Option<Item>,
    doc: Option<Value>,
}

impl Entry {
    fn disable(&mut self, e: PluginError) {
        self.state = State::Disabled;
        self.error = Some(e);
        self.item = None;
    }

    /// `id@version` or the file name.
    pub fn label(&self) -> String {
        match (&self.id, self.version) {
            (Some(i), Some(v)) => format!("{i}@{v}"),
            (Some(i), None) => i.clone(),
            _ => self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        }
    }
}

/// The plug-in set after loading.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub entries: Vec<Entry>,
    /// (format, id) -> index of the selected entry.
    pub selected: BTreeMap<(String, String), usize>,
    /// Visible notes (pin fallbacks, missing user folder, duplicates...).
    pub notes: Vec<String>,
    /// Non-empty = the startup-error state.
    pub startup_errors: Vec<String>,
    /// (format, id) whose selection is an accepted explicit pin (may be a non-active, non-withdrawn version).
    pub pinned: BTreeSet<(String, String)>,
}

const KNOWN_FORMATS: [&str; 10] = [
    MODEL_FORMAT,
    TRANSFER_FORMAT,
    GOLDEN_SPECTRA_FORMAT,
    BANDS_FORMAT,
    NOISE_GAINS_FORMAT,
    INSTRUMENT_FORMAT,
    CATALOG_FORMAT,
    ENGINE_CHECK_FORMAT,
    PROFILE_FORMAT,
    REFERENCE_SET_FORMAT,
];

/// `*.json` files under `dir`, recursively, sorted; hidden files, `._*` and symlinked/junction directories
/// are skipped.
pub fn discover(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(dir, &mut out, &mut BTreeSet::new())?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>) -> std::io::Result<()> {
    let canon = std::fs::canonicalize(dir)?;
    if !seen.insert(canon) {
        return Ok(());
    }
    let mut items: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    items.sort_by_key(|e| e.file_name());
    for e in items {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        let path = e.path();
        if ft.is_symlink() || crate::read::is_link_or_junction(&path) {
            continue; // never follow links: no cycles, no escape from the plug-in folder
        }
        if ft.is_dir() {
            walk(&path, out, seen)?;
        } else if ft.is_file()
            && path
                .extension()
                .is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case("json"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn identity(
    v: &Value,
) -> (
    Option<String>,
    Option<String>,
    Option<Version>,
    Option<PluginStatus>,
) {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    (
        s("format"),
        s("id"),
        s("version").and_then(|x| Version::parse(&x)),
        s("status").and_then(|x| PluginStatus::parse(&x)),
    )
}

fn parse_item(fmt: &str, doc: &Node, sha: &str, base: &Path) -> Result<Item, PluginError> {
    Ok(match fmt {
        MODEL_FORMAT => Item::Model(Box::new(parse_model(doc)?)),
        TRANSFER_FORMAT => Item::Transfer(Box::new(parse_transfer(doc, sha)?)),
        BANDS_FORMAT => Item::Bands(Box::new(parse_bands(doc)?)),
        NOISE_GAINS_FORMAT => Item::NoiseGains(Box::new(parse_noise_gains(doc)?)),
        INSTRUMENT_FORMAT => Item::Instruments(Box::new(parse_instruments(doc)?)),
        ENGINE_CHECK_FORMAT => Item::EngineCheck(Box::new(parse_engine_check(doc, base)?)),
        PROFILE_FORMAT => Item::Profile(Box::new(parse_profile(doc)?)),
        REFERENCE_SET_FORMAT => Item::ReferenceSet(Box::new(parse_reference_set(doc)?)),
        GOLDEN_SPECTRA_FORMAT => {
            let g = parse_golden_spectra(doc, sha)?;
            Item::GoldenSpectra {
                id: g.id,
                spectra: g.spectra.len(),
            }
        }
        CATALOG_FORMAT => {
            if doc.req("format_version")?.v.as_u64() != Some(1) {
                return Err(PluginError::schema("unsupported format_version"));
            }
            doc.req("created")?.str()?;
            let es = doc.req("entries")?.arr()?;
            if es.is_empty() {
                return Err(doc.error("entries: at least one"));
            }
            let mut entries = Vec::new();
            for e in &es {
                let f = e.req("file")?.str()?.to_string();
                let s = e.req("sha256")?.str()?.to_string();
                e.req("format")?.str()?;
                if !is_sha256_hex(&s) {
                    return Err(e.error("sha256 must be 64 lower-case hex characters"));
                }
                entries.push((f, s));
            }
            Item::Catalog { entries }
        }
        other => {
            return Err(PluginError::schema(format!("unknown format {other:?}")));
        }
    })
}

/// Load one file into an entry (no goldens yet).
fn load_entry(path: &Path, origin: Origin) -> Entry {
    let mut e = Entry {
        path: path.to_path_buf(),
        origin,
        sha256: String::new(),
        bytes: 0,
        format: None,
        id: None,
        version: None,
        status: None,
        state: State::Loaded,
        error: None,
        golden: None,
        golden_deferred: None,
        notes: Vec::new(),
        item: None,
        doc: None,
    };
    let (mut v, sha, bytes) = match read_json_file(path) {
        Ok(x) => x,
        Err(err) => {
            e.disable(err);
            return e;
        }
    };
    e.sha256 = sha;
    e.bytes = bytes;
    let (fmt, id, version, status) = identity(&v);
    e.format = fmt.clone();
    e.id = id;
    e.version = version;
    e.status = status;
    let Some(fmt) = fmt.filter(|f| KNOWN_FORMATS.contains(&f.as_str())) else {
        e.state = State::Ignored;
        e.notes.push(match e.format.as_deref() {
            None => "not a plug-in file (no \"format\")".into(),
            Some(f) => format!("unknown plug-in format {f:?} (needs a newer SPYDER Bone?)"),
        });
        return e;
    };
    let base = path.parent().unwrap_or(Path::new("."));
    if let Err(err) = resolve_sidecars(&mut v, base) {
        e.disable(err);
        return e;
    }
    match parse_item(&fmt, &Node::root(&v), &e.sha256, base) {
        Ok(item) => {
            if let Item::EngineCheck(c) = &item {
                e.golden_deferred = Some(c.golden_cases);
            }
            if let Item::Profile(p) = &item {
                e.golden_deferred = Some(p.golden_cases);
            }
            e.item = Some(item);
            e.doc = Some(v);
        }
        Err(err) => e.disable(err),
    }
    e
}

/// Golden spectra file named by a golden block, loaded once per canonical path.
fn golden_spectra<'c>(
    entry_path: &Path,
    doc: &Value,
    cache: &'c mut BTreeMap<PathBuf, Result<GoldenSpectra, PluginError>>,
) -> Result<&'c GoldenSpectra, PluginError> {
    let base = entry_path.parent().unwrap_or(Path::new("."));
    let name = Node::root(doc)
        .req("golden")?
        .req("spectra_file")?
        .str()?
        .to_string();
    let p = package_path(base, &name, "golden spectra file")?;
    let r = cache
        .entry(p.clone())
        .or_insert_with(|| load_golden_spectra(&p));
    r.as_ref()
        .map_err(|e| PluginError::new(e.kind, format!("golden spectra file {name}: {}", e.message)))
}

impl Registry {
    /// Scan the locations, validate every file, run the goldens, apply cross-file rules and precedence.
    pub fn load(locations: &[Location], pins: &Pins) -> Registry {
        let mut reg = Registry::default();
        for loc in locations {
            if !loc.dir.is_dir() {
                match loc.origin {
                    Origin::Bundled => reg.startup_errors.push(format!(
                        "the bundled plug-in folder {} is missing",
                        loc.dir.display()
                    )),
                    Origin::User => reg.notes.push(format!(
                        "user plug-in folder {} not found (nothing added)",
                        loc.dir.display()
                    )),
                }
                continue;
            }
            match discover(&loc.dir) {
                Ok(files) => {
                    for f in files {
                        reg.entries.push(load_entry(&f, loc.origin));
                    }
                }
                Err(e) => reg
                    .notes
                    .push(format!("cannot list {}: {e}", loc.dir.display())),
            }
        }
        reg.run_goldens();
        reg.conflicts();
        reg.cross_file();
        reg.resolve(pins);
        reg.cross_file_selected();
        reg.resolve(pins);
        reg.run_check_and_profile_goldens();
        reg.resolve(pins);
        reg.startup_state();
        for e in &mut reg.entries {
            e.doc = None;
        }
        reg
    }

    fn run_goldens(&mut self) {
        let mut cache = BTreeMap::new();
        let mut results: Vec<(usize, Result<GoldenRun, PluginError>)> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.state != State::Loaded {
                continue;
            }
            let (Some(item), Some(doc)) = (&e.item, &e.doc) else {
                continue;
            };
            let node = Node::root(doc);
            let r = match item {
                Item::Model(m) => golden_spectra(&e.path, doc, &mut cache)
                    .and_then(|gs| run_model_goldens(m, &node, gs)),
                Item::Transfer(t) => golden_spectra(&e.path, doc, &mut cache)
                    .and_then(|gs| run_transfer_goldens(t, &node, gs)),
                Item::Bands(_) | Item::NoiseGains(_) => {
                    let folder = e.path.parent();
                    let transfers: BTreeMap<String, &Transfer> = self
                        .entries
                        .iter()
                        .filter(|o| o.path.parent() == folder)
                        .filter_map(|o| match &o.item {
                            Some(Item::Transfer(t)) => Some((o.sha256.clone(), t.as_ref())),
                            _ => None,
                        })
                        .collect();
                    let table = match item {
                        Item::Bands(b) => TableRef::Bands(b),
                        Item::NoiseGains(n) => TableRef::Noise(n),
                        _ => continue,
                    };
                    golden_spectra(&e.path, doc, &mut cache)
                        .and_then(|gs| run_table_goldens(&node, table, gs, &transfers))
                }
                _ => continue,
            };
            results.push((i, r));
        }
        for (i, r) in results {
            let e = &mut self.entries[i];
            match r {
                Ok(run) if run.passed() => e.golden = Some(run),
                Ok(run) => {
                    let f = &run.failures[0];
                    let msg = format!(
                        "golden test failed: {} of {} checks, first at case {} {}: got {} want {}",
                        run.failures.len(),
                        run.checks,
                        f.case,
                        f.key,
                        f.got,
                        f.want
                    );
                    e.golden = Some(run);
                    e.disable(PluginError::new(ErrorKind::GoldenFailed, msg));
                }
                Err(err) => e.disable(err),
            }
        }
    }

    /// Same (format, id, version) with different bytes: all rejected. Identical bytes: keep the first.
    fn conflicts(&mut self) {
        let mut groups: BTreeMap<(String, String, Version), Vec<usize>> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.state == State::Ignored {
                continue;
            }
            if let (Some(f), Some(id), Some(v)) = (&e.format, &e.id, e.version) {
                groups
                    .entry((f.clone(), id.clone(), v))
                    .or_default()
                    .push(i);
            }
        }
        for ((_, id, v), idx) in groups {
            if idx.len() < 2 {
                continue;
            }
            let shas: BTreeSet<&str> = idx
                .iter()
                .map(|&i| self.entries[i].sha256.as_str())
                .collect();
            if shas.len() > 1 {
                let files: Vec<String> = idx
                    .iter()
                    .map(|&i| self.entries[i].path.display().to_string())
                    .collect();
                for &i in &idx {
                    self.entries[i].disable(PluginError::new(
                        ErrorKind::Conflict,
                        format!(
                            "{id}@{v} exists with different contents in {} files ({}); all are rejected",
                            files.len(),
                            files.join(", ")
                        ),
                    ));
                }
            } else {
                // Codex Phase 2 MEDIUM 5: keep the copy that loaded (a copy can fail for folder-dependent
                // reasons, e.g. its golden spectra file is missing there); the first one otherwise
                let first = idx
                    .iter()
                    .copied()
                    .find(|&i| self.entries[i].state == State::Loaded)
                    .unwrap_or(idx[0]);
                let fp = self.entries[first].path.display().to_string();
                for &i in idx.iter().filter(|&&i| i != first) {
                    let e = &mut self.entries[i];
                    e.state = State::Ignored;
                    e.item = None;
                    e.notes.push(format!("byte-identical duplicate of {fp}"));
                }
            }
        }
    }

    fn loaded(&self, i: usize) -> bool {
        self.entries[i].state == State::Loaded
    }

    /// Cross-file rules that do not depend on precedence.
    fn cross_file(&mut self) {
        let mut disable: Vec<(usize, PluginError)> = Vec::new();
        // consensus components: a loaded model with this id, version and file sha256
        for (i, e) in self.entries.iter().enumerate() {
            let Some(Item::Model(m)) = &e.item else {
                continue;
            };
            let Body::Consensus { components } = &m.body else {
                continue;
            };
            for c in components {
                let ok = self.entries.iter().enumerate().any(|(j, o)| {
                    self.loaded(j)
                        && o.sha256 == c.sha256
                        && matches!(&o.item, Some(Item::Model(om)) if om.header.id == c.id && om.header.version == c.version)
                });
                if !ok {
                    disable.push((
                        i,
                        PluginError::new(
                            ErrorKind::CrossFile,
                            format!(
                                "consensus component {} ({}@{}, sha256 {}) does not match any loaded model file",
                                c.name,
                                c.id,
                                c.version,
                                super::short(&c.sha256, 12)
                            ),
                        ),
                    ));
                    break;
                }
            }
        }
        // transfer keys of bands / noise gains: transfer files in the same folder (by file sha256)
        for (i, e) in self.entries.iter().enumerate() {
            let keys: Vec<&String> = match &e.item {
                Some(Item::NoiseGains(n)) => n.models.values().flat_map(|m| m.keys()).collect(),
                Some(Item::Bands(b)) => b
                    .bands
                    .values()
                    .filter_map(|x| x.readability.as_ref())
                    .flat_map(|r| r.gain_e_per_n2.keys())
                    .collect(),
                _ => continue,
            };
            let folder = e.path.parent();
            for k in keys {
                if k == NO_TRANSFER {
                    continue;
                }
                let found = self.entries.iter().any(|o| {
                    o.path.parent() == folder
                        && o.sha256 == *k
                        && o.format.as_deref() == Some(TRANSFER_FORMAT)
                });
                if !found {
                    disable.push((
                        i,
                        PluginError::new(
                            ErrorKind::CrossFile,
                            format!(
                                "transfer key {} matches no transfer file in this folder (was a transfer file changed?)",
                                super::short(k, 12)
                            ),
                        ),
                    ));
                    break;
                }
            }
        }
        // catalog entries: file present (relative to the catalog) with this sha256
        for (i, e) in self.entries.iter().enumerate() {
            let Some(Item::Catalog { entries }) = &e.item else {
                continue;
            };
            let base = e.path.parent().unwrap_or(Path::new("."));
            let bad: Vec<String> = entries
                .iter()
                .filter(|(f, s)| {
                    package_path(base, f, "catalog entry")
                        .ok()
                        .and_then(|p| std::fs::read(p).ok())
                        .map(|b| super::sha256_hex(&b) != *s)
                        .unwrap_or(true)
                })
                .map(|(f, _)| f.clone())
                .collect();
            if !bad.is_empty() {
                disable.push((
                    i,
                    PluginError::new(
                        ErrorKind::CrossFile,
                        format!(
                            "catalog: sha256 mismatch or file missing: {}",
                            bad.join(", ")
                        ),
                    ),
                ));
            }
        }
        for (i, err) in disable {
            if self.entries[i].state == State::Loaded {
                self.entries[i].disable(err);
            }
        }
    }

    /// Cross-file rules against the selected set (bands named by checks, models named by profiles).
    fn cross_file_selected(&mut self) {
        let band_names: BTreeSet<String> = self
            .bands()
            .map(|b| b.bands.keys().cloned().collect())
            .unwrap_or_default();
        let model_ids: BTreeSet<String> =
            self.models().iter().map(|m| m.header.id.clone()).collect();
        let mut disable = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            match &e.item {
                Some(Item::EngineCheck(c)) => {
                    let missing: Vec<String> = c
                        .params
                        .band_refs()
                        .into_iter()
                        .filter(|b| !band_names.contains(b))
                        .collect();
                    if !missing.is_empty() {
                        disable.push((
                            i,
                            PluginError::new(
                                ErrorKind::CrossFile,
                                format!(
                                    "bands not in the loaded band table: {}",
                                    missing.join(", ")
                                ),
                            ),
                        ));
                    }
                }
                Some(Item::Profile(p)) => {
                    let missing: Vec<&str> = p
                        .models
                        .iter()
                        .map(|m| m.model_id.as_str())
                        .filter(|id| !model_ids.contains(*id))
                        .collect();
                    if !missing.is_empty() {
                        disable.push((
                            i,
                            PluginError::new(
                                ErrorKind::CrossFile,
                                format!("models not loaded: {}", missing.join(", ")),
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
        for (i, err) in disable {
            if self.entries[i].state == State::Loaded {
                self.entries[i].disable(err);
            }
        }
    }

    /// Goldens of engine checks and analysis profiles (they read the selected band table and each other).
    fn run_check_and_profile_goldens(&mut self) {
        use crate::pipeline::checkgold::{run_check_goldens, run_profile_goldens, CheckEnv};
        let env = CheckEnv {
            bands: self.bands(),
            checks: self
                .checks()
                .into_iter()
                .map(|c| (c.check.clone(), &c.params))
                .collect(),
        };
        let mut cache = BTreeMap::new();
        let mut results: Vec<(usize, Result<GoldenRun, PluginError>)> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.state != State::Loaded {
                continue;
            }
            let (Some(item), Some(doc)) = (&e.item, &e.doc) else {
                continue;
            };
            let node = Node::root(doc);
            let r = match item {
                Item::EngineCheck(c) => golden_spectra(&e.path, doc, &mut cache)
                    .and_then(|gs| run_check_goldens(c, &node, &env, gs)),
                Item::Profile(p) => run_profile_goldens(p, &node),
                _ => continue,
            };
            results.push((i, r));
        }
        for (i, r) in results {
            let e = &mut self.entries[i];
            e.golden_deferred = None;
            match r {
                Ok(run) if run.passed() => e.golden = Some(run),
                Ok(run) => {
                    let f = &run.failures[0];
                    let msg = format!(
                        "golden test failed: {} of {} checks, first at case {} {}: got {} want {}",
                        run.failures.len(),
                        run.checks,
                        f.case,
                        f.key,
                        f.got,
                        f.want
                    );
                    e.golden = Some(run);
                    e.disable(PluginError::new(ErrorKind::GoldenFailed, msg));
                }
                Err(err) => e.disable(err),
            }
        }
    }

    /// Precedence and pins per (format, id).
    fn resolve(&mut self, pins: &Pins) {
        self.selected.clear();
        self.pinned.clear();
        self.notes.retain(|n| !n.starts_with("pin: "));
        let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.state == State::Ignored {
                continue;
            }
            if let (Some(f), Some(id)) = (&e.format, &e.id) {
                if e.version.is_some() {
                    groups.entry((f.clone(), id.clone())).or_default().push(i);
                }
            }
        }
        for ((f, id), idx) in groups {
            let best_active = idx
                .iter()
                .copied()
                .filter(|&i| self.loaded(i) && self.entries[i].status == Some(PluginStatus::Active))
                .max_by_key(|&i| self.entries[i].version);
            let choice = match pins.get(&id) {
                None => best_active,
                Some(pv) => {
                    let pinned = idx
                        .iter()
                        .copied()
                        .find(|&i| self.entries[i].version == Some(*pv));
                    match pinned {
                        Some(i)
                            if self.loaded(i)
                                && self.entries[i].status != Some(PluginStatus::Withdrawn) =>
                        {
                            self.pinned.insert((f.clone(), id.clone()));
                            Some(i)
                        }
                        other => {
                            let why = match other {
                                None => "is not installed".to_string(),
                                Some(i)
                                    if self.entries[i].status == Some(PluginStatus::Withdrawn) =>
                                {
                                    "is withdrawn".to_string()
                                }
                                Some(i) => format!(
                                    "failed to load ({})",
                                    self.entries[i]
                                        .error
                                        .as_ref()
                                        .map(|e| e.message.as_str())
                                        .unwrap_or("disabled")
                                ),
                            };
                            let using = best_active
                                .and_then(|i| self.entries[i].version)
                                .map(|v| format!("using {v}"))
                                .unwrap_or_else(|| "no other version is available".into());
                            self.notes.push(format!("pin: {id}@{pv} {why}; {using}"));
                            best_active
                        }
                    }
                }
            };
            if let Some(i) = choice {
                self.selected.insert((f, id), i);
            }
        }
    }

    fn startup_state(&mut self) {
        for e in &self.entries {
            let bundled_failure = e.origin == Origin::Bundled
                && e.state == State::Disabled
                && (e.status == Some(PluginStatus::Active)
                    || e.format.as_deref() == Some(CATALOG_FORMAT)
                    || e.format.as_deref() == Some(GOLDEN_SPECTRA_FORMAT));
            if bundled_failure {
                self.startup_errors.push(format!(
                    "bundled {} ({}) failed: {}",
                    e.label(),
                    e.path.display(),
                    e.error.as_ref().map(|x| x.message.as_str()).unwrap_or("")
                ));
            }
        }
        let has_verdict = self.profiles().iter().any(|p| {
            p.verdict_model_id()
                .is_some_and(|id| self.model(id).is_some())
        });
        if !has_verdict {
            self.startup_errors.push(
                "no verdict model available (no loaded analysis profile names a loaded verdict-input model)"
                    .into(),
            );
        }
    }

    fn selected_items(&self, format: &str) -> impl Iterator<Item = &Entry> + '_ {
        let format = format.to_string();
        self.selected
            .iter()
            .filter(move |((f, _), _)| *f == format)
            .map(|(_, &i)| &self.entries[i])
            .filter(|e| e.state == State::Loaded)
    }

    /// Selected, loaded models (regression and consensus), in id order.
    pub fn models(&self) -> Vec<&Model> {
        self.selected_items(MODEL_FORMAT)
            .filter_map(|e| match &e.item {
                Some(Item::Model(m)) => Some(m.as_ref()),
                _ => None,
            })
            .collect()
    }

    /// The selected model with this id.
    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models().into_iter().find(|m| m.header.id == id)
    }

    /// The entry (file) a selected item came from, by format and id.
    pub fn selected_entry(&self, format: &str, id: &str) -> Option<&Entry> {
        self.selected
            .get(&(format.to_string(), id.to_string()))
            .map(|&i| &self.entries[i])
    }

    /// Ids of transfers selected by an accepted explicit pin (selection may use them even if not active).
    pub fn pinned_transfer_ids(&self) -> Vec<String> {
        self.pinned
            .iter()
            .filter(|(f, _)| f == TRANSFER_FORMAT)
            .map(|(_, id)| id.clone())
            .collect()
    }

    /// Selected transfers (one per id).
    pub fn transfers(&self) -> Vec<&Transfer> {
        self.selected_items(TRANSFER_FORMAT)
            .filter_map(|e| match &e.item {
                Some(Item::Transfer(t)) => Some(t.as_ref()),
                _ => None,
            })
            .collect()
    }

    pub fn bands(&self) -> Option<&Bands> {
        self.selected_items(BANDS_FORMAT)
            .find_map(|e| match &e.item {
                Some(Item::Bands(b)) => Some(b.as_ref()),
                _ => None,
            })
    }

    pub fn noise_gains(&self) -> Option<&NoiseGains> {
        self.selected_items(NOISE_GAINS_FORMAT)
            .find_map(|e| match &e.item {
                Some(Item::NoiseGains(b)) => Some(b.as_ref()),
                _ => None,
            })
    }

    pub fn instruments(&self) -> Option<&Instruments> {
        self.selected_items(INSTRUMENT_FORMAT)
            .find_map(|e| match &e.item {
                Some(Item::Instruments(b)) => Some(b.as_ref()),
                _ => None,
            })
    }

    pub fn profiles(&self) -> Vec<&AnalysisProfile> {
        self.selected_items(PROFILE_FORMAT)
            .filter_map(|e| match &e.item {
                Some(Item::Profile(p)) => Some(p.as_ref()),
                _ => None,
            })
            .collect()
    }

    pub fn checks(&self) -> Vec<&EngineCheck> {
        self.selected_items(ENGINE_CHECK_FORMAT)
            .filter_map(|e| match &e.item {
                Some(Item::EngineCheck(c)) => Some(c.as_ref()),
                _ => None,
            })
            .collect()
    }

    pub fn reference_sets(&self) -> Vec<&ReferenceSet> {
        self.selected_items(REFERENCE_SET_FORMAT)
            .filter_map(|e| match &e.item {
                Some(Item::ReferenceSet(r)) => Some(r.as_ref()),
                _ => None,
            })
            .collect()
    }

    /// Whether the plug-in set is in the startup-error state.
    pub fn startup_error(&self) -> bool {
        !self.startup_errors.is_empty()
    }

    /// Entries that are plug-ins (not ignored files).
    pub fn plugin_entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.state != State::Ignored)
    }
}
