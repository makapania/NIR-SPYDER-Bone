//! Strict JSON loading for plug-ins, `.npy` sidecar resolution with path confinement, and the [`Node`]
//! accessor used for hand-written schema validation.
//!
//! * Numbers parse with correct rounding (`serde_json` `float_roundtrip`); `NaN`, `Infinity` and numbers that
//!   overflow to infinity (e.g. `1e999`) are rejected by the parser.
//! * Sidecars (`{"npy": "name.npy", "sha256": "..."}`, exactly these two keys) are replaced by their arrays.
//!   The name must be a plain relative path inside the plug-in's own folder (no absolute path, drive, backslash,
//!   `.`/`..` segment, or symlink/junction escaping the folder), the file at most 16 MiB, matching its SHA-256,
//!   little-endian float64, C order, 1-D or 2-D, non-empty and finite. Never pickles.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{
    npy, sha256_hex, ErrorKind, PResult, PluginError, PLUGIN_MAX_BYTES, SIDECAR_MAX_BYTES,
};

/// A JSON value with its path (for messages like `$.regression.coefficients[3]: must be a number`).
#[derive(Debug, Clone)]
pub struct Node<'a> {
    pub v: &'a Value,
    path: String,
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

impl<'a> Node<'a> {
    pub fn root(v: &'a Value) -> Self {
        Node {
            v,
            path: "$".to_string(),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn err(&self, what: impl AsRef<str>) -> PluginError {
        PluginError::schema(format!("{}: {}", self.path, what.as_ref()))
    }

    /// A schema error at this node.
    pub fn error(&self, what: impl AsRef<str>) -> PluginError {
        self.err(what)
    }

    /// Child by key; None when missing or null (an optional field).
    pub fn opt(&self, key: &str) -> Option<Node<'a>> {
        match self.v.get(key) {
            None | Some(Value::Null) => None,
            Some(v) => Some(Node {
                v,
                path: format!("{}.{}", self.path, key),
            }),
        }
    }

    /// `true` when the key is present (even as null).
    pub fn has(&self, key: &str) -> bool {
        self.v.as_object().is_some_and(|m| m.contains_key(key))
    }

    /// Required child (missing or null is an error).
    pub fn req(&self, key: &str) -> PResult<Node<'a>> {
        if !self.v.is_object() {
            return Err(self.err(format!("must be an object (looking for {key:?})")));
        }
        self.opt(key)
            .ok_or_else(|| self.err(format!("missing required field {key:?}")))
    }

    pub fn obj(&self) -> PResult<&'a Map<String, Value>> {
        self.v
            .as_object()
            .ok_or_else(|| self.err(format!("must be an object, not {}", type_name(self.v))))
    }

    /// The object's entries in file order... serde_json maps are sorted by key (deterministic).
    pub fn entries(&self) -> PResult<Vec<(String, Node<'a>)>> {
        Ok(self
            .obj()?
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    Node {
                        v,
                        path: format!("{}.{}", self.path, k),
                    },
                )
            })
            .collect())
    }

    pub fn str(&self) -> PResult<&'a str> {
        self.v
            .as_str()
            .ok_or_else(|| self.err(format!("must be a string, not {}", type_name(self.v))))
    }

    pub fn bool(&self) -> PResult<bool> {
        self.v
            .as_bool()
            .ok_or_else(|| self.err(format!("must be true or false, not {}", type_name(self.v))))
    }

    /// A finite number.
    pub fn f64(&self) -> PResult<f64> {
        match self.v.as_f64() {
            Some(x) if x.is_finite() => Ok(x),
            _ => Err(self.err(format!(
                "must be a finite number, not {}",
                type_name(self.v)
            ))),
        }
    }

    /// A non-negative integer (integral floats such as 31.0 are accepted, as Python's int() would).
    pub fn u64(&self) -> PResult<u64> {
        if let Some(u) = self.v.as_u64() {
            return Ok(u);
        }
        match self.v.as_f64() {
            Some(f) if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 9.0e15 => Ok(f as u64),
            _ => Err(self.err("must be a non-negative integer")),
        }
    }

    pub fn usize(&self) -> PResult<usize> {
        usize::try_from(self.u64()?).map_err(|_| self.err("integer too large"))
    }

    pub fn arr(&self) -> PResult<Vec<Node<'a>>> {
        let a = self
            .v
            .as_array()
            .ok_or_else(|| self.err(format!("must be a list, not {}", type_name(self.v))))?;
        Ok(a.iter()
            .enumerate()
            .map(|(i, v)| Node {
                v,
                path: format!("{}[{}]", self.path, i),
            })
            .collect())
    }

    /// A list of finite numbers.
    pub fn vec_f64(&self) -> PResult<Vec<f64>> {
        self.arr()?.iter().map(|n| n.f64()).collect()
    }

    /// A list of finite numbers of exactly `n` elements.
    pub fn vec_f64_len(&self, n: usize) -> PResult<Vec<f64>> {
        let v = self.vec_f64()?;
        if v.len() != n {
            return Err(self.err(format!("expected {n} numbers, found {}", v.len())));
        }
        Ok(v)
    }

    /// `[lo, hi]` with lo < hi (or lo <= hi when `allow_equal`).
    pub fn range(&self, allow_equal: bool) -> PResult<(f64, f64)> {
        let v = self.vec_f64_len(2)?;
        if v[0] < v[1] || (allow_equal && v[0] == v[1]) {
            Ok((v[0], v[1]))
        } else {
            Err(self.err("must be [lo, hi] with lo < hi"))
        }
    }

    pub fn vec_str(&self) -> PResult<Vec<String>> {
        self.arr()?
            .iter()
            .map(|n| n.str().map(str::to_string))
            .collect()
    }

    /// A string from an allowed set.
    pub fn one_of(&self, allowed: &[&str]) -> PResult<&'a str> {
        let s = self.str()?;
        if allowed.contains(&s) {
            Ok(s)
        } else {
            Err(self.err(format!("{s:?} is not one of {allowed:?}")))
        }
    }
}

/// Read a plug-in file: bytes (size-capped), file SHA-256, parsed JSON.
pub fn read_json_file(path: &Path) -> PResult<(Value, String, u64)> {
    let meta = std::fs::metadata(path)
        .map_err(|e| PluginError::new(ErrorKind::Io, format!("cannot read file: {e}")))?;
    if meta.len() > PLUGIN_MAX_BYTES {
        return Err(PluginError::new(
            ErrorKind::Io,
            format!("file is {} bytes; limit {PLUGIN_MAX_BYTES}", meta.len()),
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| PluginError::new(ErrorKind::Io, format!("cannot read file: {e}")))?;
    let sha = sha256_hex(&bytes);
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| {
        PluginError::new(
            ErrorKind::Json,
            format!("not valid JSON (or a non-finite number): {e}"),
        )
    })?;
    Ok((v, sha, bytes.len() as u64))
}

/// Resolve a file name given inside a plug-in against the plug-in's own folder, refusing anything that could
/// leave it: absolute paths, drive letters, backslashes, empty, `.` or `..` segments, and symlinks or junctions
/// resolving outside (spyder_ref `package_path`).
pub fn package_path(base: &Path, name: &str, what: &str) -> PResult<PathBuf> {
    let kind = if what == "sidecar" {
        ErrorKind::Sidecar
    } else {
        ErrorKind::Schema
    };
    let bad = |m: String| PluginError::new(kind, m);
    if name.is_empty() || name.contains('\\') || name.contains(':') || name.contains('\0') {
        return Err(bad(format!(
            "{what} path {name:?} is not a plain relative path"
        )));
    }
    if name.starts_with('/')
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(bad(format!(
            "{what} path {name:?} must stay inside the package folder"
        )));
    }
    let root = std::fs::canonicalize(base)
        .map_err(|e| bad(format!("{what}: package folder unavailable: {e}")))?;
    let f = std::fs::canonicalize(base.join(name))
        .map_err(|_| bad(format!("{what} {name:?} not found in the package folder")))?;
    if f == root || !f.starts_with(&root) {
        return Err(bad(format!(
            "{what} path {name:?} resolves outside the package folder"
        )));
    }
    if !f.is_file() {
        return Err(bad(format!(
            "{what} {name:?} not found in the package folder"
        )));
    }
    Ok(f)
}

/// Load one sidecar: confined path, size cap, SHA-256 of the FILE, `.npy` float64 LE C-order 1-D/2-D, finite.
pub fn load_sidecar(base: &Path, name: &str, sha256: &str) -> PResult<npy::Array> {
    let sc = |m: String| PluginError::new(ErrorKind::Sidecar, m);
    if !name.ends_with(".npy") {
        return Err(sc(format!("sidecar {name:?} is not a .npy file")));
    }
    let f = package_path(base, name, "sidecar")?;
    let len = std::fs::metadata(&f)
        .map_err(|e| sc(format!("sidecar {name}: {e}")))?
        .len();
    if len > SIDECAR_MAX_BYTES {
        return Err(sc(format!(
            "sidecar {name} is {len} bytes; limit {SIDECAR_MAX_BYTES}"
        )));
    }
    let bytes = std::fs::read(&f).map_err(|e| sc(format!("sidecar {name}: {e}")))?;
    if sha256_hex(&bytes) != sha256 {
        return Err(sc(format!("sidecar {name} sha256 mismatch")));
    }
    let a = npy::parse(&bytes).map_err(|e| sc(format!("sidecar {name}: {e}")))?;
    if a.data.is_empty() {
        return Err(sc(format!(
            "sidecar {name} must be a non-empty vector or matrix"
        )));
    }
    if a.data.iter().any(|v| !v.is_finite()) {
        return Err(sc(format!("sidecar {name} holds non-finite values")));
    }
    Ok(a)
}

fn is_sidecar_ref(m: &Map<String, Value>) -> bool {
    m.len() == 2 && m.contains_key("npy") && m.contains_key("sha256")
}

/// Replace every `{"npy": ..., "sha256": ...}` object by its array (spyder_ref `_resolve_sidecars`).
pub fn resolve_sidecars(v: &mut Value, base: &Path) -> PResult<()> {
    match v {
        Value::Object(m) if is_sidecar_ref(m) => {
            let name = m["npy"].as_str().ok_or_else(|| {
                PluginError::new(ErrorKind::Sidecar, "sidecar \"npy\" must be a file name")
            })?;
            let sha = m["sha256"].as_str().ok_or_else(|| {
                PluginError::new(ErrorKind::Sidecar, "sidecar \"sha256\" must be a string")
            })?;
            let a = load_sidecar(base, name, sha)?;
            *v = a.to_json();
            Ok(())
        }
        Value::Object(m) => {
            for (_, x) in m.iter_mut() {
                resolve_sidecars(x, base)?;
            }
            Ok(())
        }
        Value::Array(a) => {
            for x in a.iter_mut() {
                resolve_sidecars(x, base)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
