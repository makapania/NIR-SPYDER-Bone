//! An ordered result-record value, mirroring the oracle's Python dicts: insertion-ordered maps, ints distinct
//! from floats, `None` as `Null`. [`flatten`] is the oracle's `export.flatten` (dotted keys; lists give `<key>.n`
//! and `<key>.<i>`), which is what every golden and parity comparison uses.

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    List(Vec<V>),
    Map(Obj),
}

/// An insertion-ordered map.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Obj(pub Vec<(String, V)>);

impl Obj {
    pub fn new() -> Self {
        Obj(Vec::new())
    }

    /// Set a key (replacing the value in place if present, as a Python dict does).
    pub fn set(&mut self, k: &str, v: impl Into<V>) -> &mut Self {
        let v = v.into();
        if let Some(e) = self.0.iter_mut().find(|(kk, _)| kk == k) {
            e.1 = v;
        } else {
            self.0.push((k.to_string(), v));
        }
        self
    }

    /// Builder form of [`Obj::set`].
    pub fn with(mut self, k: &str, v: impl Into<V>) -> Self {
        self.set(k, v);
        self
    }

    pub fn get(&self, k: &str) -> Option<&V> {
        self.0.iter().find(|(kk, _)| kk == k).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, k: &str) -> Option<&mut V> {
        self.0.iter_mut().find(|(kk, _)| kk == k).map(|(_, v)| v)
    }

    /// Append every entry of `o` (Python `dict.update`).
    pub fn update(&mut self, o: Obj) {
        for (k, v) in o.0 {
            self.set(&k, v);
        }
    }
}

impl V {
    pub fn as_obj(&self) -> Option<&Obj> {
        match self {
            V::Map(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            V::Num(x) => Some(*x),
            V::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            V::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, V::Null)
    }
    /// Python truthiness of the values the oracle tests (`if x.get("fired")`).
    pub fn truthy(&self) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => *b,
            V::Int(i) => *i != 0,
            V::Num(x) => *x != 0.0,
            V::Str(s) => !s.is_empty(),
            V::List(l) => !l.is_empty(),
            V::Map(m) => !m.0.is_empty(),
        }
    }
}

impl From<f64> for V {
    fn from(x: f64) -> V {
        V::Num(x)
    }
}
impl From<i64> for V {
    fn from(x: i64) -> V {
        V::Int(x)
    }
}
impl From<usize> for V {
    fn from(x: usize) -> V {
        V::Int(x as i64)
    }
}
impl From<bool> for V {
    fn from(x: bool) -> V {
        V::Bool(x)
    }
}
impl From<&str> for V {
    fn from(x: &str) -> V {
        V::Str(x.to_string())
    }
}
impl From<String> for V {
    fn from(x: String) -> V {
        V::Str(x)
    }
}
impl From<Obj> for V {
    fn from(x: Obj) -> V {
        V::Map(x)
    }
}
impl From<Vec<V>> for V {
    fn from(x: Vec<V>) -> V {
        V::List(x)
    }
}
impl<T: Into<V>> From<Option<T>> for V {
    fn from(x: Option<T>) -> V {
        x.map_or(V::Null, Into::into)
    }
}

/// The oracle's `_f`: a finite float, else None.
pub fn fin(x: f64) -> V {
    if x.is_finite() {
        V::Num(x)
    } else {
        V::Null
    }
}

/// A list of strings.
pub fn strs<S: AsRef<str>>(v: &[S]) -> V {
    V::List(v.iter().map(|s| V::Str(s.as_ref().to_string())).collect())
}

/// The oracle's `export.flatten`.
pub fn flatten(v: &V) -> Vec<(String, V)> {
    let mut out = Vec::new();
    flat_into(v, "", &mut out);
    out
}

fn flat_into(v: &V, prefix: &str, out: &mut Vec<(String, V)>) {
    match v {
        V::Map(o) => {
            for (k, x) in &o.0 {
                flat_into(x, &format!("{prefix}{k}."), out);
            }
        }
        V::List(l) => {
            let key = &prefix[..prefix.len().saturating_sub(1)];
            out.push((format!("{key}.n"), V::Int(l.len() as i64)));
            for (i, x) in l.iter().enumerate() {
                flat_into(x, &format!("{key}.{i}."), out);
            }
        }
        scalar => {
            let key = &prefix[..prefix.len().saturating_sub(1)];
            out.push((key.to_string(), scalar.clone()));
        }
    }
}

/// Python `json.dumps(v)` (default separators; floats by `repr`; NaN as `NaN`); `sort_keys` sorts map keys.
pub fn py_json(v: &V, sort_keys: bool) -> String {
    use crate::pyfmt::{float_repr, json_str};
    match v {
        V::Null => "null".into(),
        V::Bool(b) => if *b { "true" } else { "false" }.into(),
        V::Int(i) => i.to_string(),
        V::Num(x) if x.is_nan() => "NaN".into(),
        V::Num(x) if x.is_infinite() => if *x > 0.0 { "Infinity" } else { "-Infinity" }.into(),
        V::Num(x) => float_repr(*x),
        V::Str(s) => json_str(s),
        V::List(l) => format!(
            "[{}]",
            l.iter()
                .map(|x| py_json(x, sort_keys))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        V::Map(o) => {
            let mut e: Vec<&(String, V)> = o.0.iter().collect();
            if sort_keys {
                e.sort_by(|a, b| a.0.cmp(&b.0));
            }
            format!(
                "{{{}}}",
                e.iter()
                    .map(|(k, x)| format!("{}: {}", json_str(k), py_json(x, sort_keys)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

impl Serialize for V {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            V::Null => s.serialize_none(),
            V::Bool(b) => s.serialize_bool(*b),
            V::Int(i) => s.serialize_i64(*i),
            V::Num(x) if x.is_finite() => s.serialize_f64(*x),
            V::Num(_) => s.serialize_none(),
            V::Str(x) => s.serialize_str(x),
            V::List(l) => {
                let mut q = s.serialize_seq(Some(l.len()))?;
                for x in l {
                    q.serialize_element(x)?;
                }
                q.end()
            }
            V::Map(o) => o.serialize(s),
        }
    }
}

impl Serialize for Obj {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

/// Compare a produced flat value with an expected JSON value as the oracle's goldens do
/// (`make_goldens.compare`): expected floats with |got - want| <= abs + rel |want|; ints, labels, booleans and
/// null exactly (an int compares equal to the same float, as in Python).
pub fn matches_expected(got: &V, want: &serde_json::Value, abs: f64, rel: f64) -> bool {
    use serde_json::Value as J;
    match want {
        J::Null => got.is_null(),
        J::Bool(b) => got.as_bool() == Some(*b),
        J::String(s) => got.as_str() == Some(s.as_str()),
        J::Number(n) if n.is_f64() => {
            let w = n.as_f64().unwrap_or(f64::NAN);
            match got {
                V::Num(g) => g.is_finite() && (g - w).abs() <= abs + rel * w.abs(),
                V::Int(g) => ((*g as f64) - w).abs() <= abs + rel * w.abs(),
                _ => false,
            }
        }
        J::Number(n) => match got {
            V::Int(g) => n.as_i64() == Some(*g),
            V::Num(g) => n.as_f64() == Some(*g),
            _ => false,
        },
        _ => false,
    }
}

/// Compare with the engine-check goldens' rule (`checkgold._cmp`): labels, booleans and null exactly; every
/// number (int or float) with the tolerance.
pub fn matches_check_expected(got: &V, want: &serde_json::Value, abs: f64, rel: f64) -> bool {
    use serde_json::Value as J;
    match want {
        J::Number(n) => {
            let w = n.as_f64().unwrap_or(f64::NAN);
            match got {
                V::Num(g) => g.is_finite() && (g - w).abs() <= abs + rel * w.abs(),
                V::Int(g) => ((*g as f64) - w).abs() <= abs + rel * w.abs(),
                _ => false,
            }
        }
        other => matches_expected(got, other, abs, rel),
    }
}
