//! Reference spectra sets (`spyder-bone/reference_set`, PLAN section 4): mean absorbance per reference level
//! (about 0, 1, 3, 6 and 10% collagen) for display. They compute nothing, so they carry no goldens; the loader
//! checks the grid, that every level has one finite absorbance per grid point, and the member count.

use super::{parse_header, Header, Node, PResult, REFERENCE_SET_FORMAT};

#[derive(Debug, Clone, PartialEq)]
pub struct RefLevel {
    pub label: String,
    pub display_label: String,
    pub n: usize,
    pub absorbance: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct ReferenceSet {
    pub header: Header,
    pub grid: (f64, f64, usize),
    pub levels: Vec<RefLevel>,
}

pub fn parse_reference_set(doc: &Node) -> PResult<ReferenceSet> {
    let header = parse_header(doc, REFERENCE_SET_FORMAT)?;
    let g = doc.req("grid")?;
    let grid = (
        g.req("start_nm")?.f64()?,
        g.req("step_nm")?.f64()?,
        g.req("n")?.usize()?,
    );
    if !(grid.1 > 0.0) || grid.2 < 2 {
        return Err(g.error("grid needs step_nm > 0 and n >= 2"));
    }
    let ls = doc.req("levels")?.arr()?;
    if ls.is_empty() {
        return Err(doc.error("levels: at least one"));
    }
    let mut levels = Vec::new();
    for l in &ls {
        let label = l.req("label")?.str()?.to_string();
        let n = l.req("n")?.usize()?;
        let members = l.req("members")?.vec_str()?;
        if members.len() != n {
            return Err(l.error(format!(
                "level {label}: {} members listed, n = {n}",
                members.len()
            )));
        }
        let absorbance = l.req("absorbance")?.vec_f64_len(grid.2)?;
        levels.push(RefLevel {
            display_label: l
                .opt("display_label")
                .map(|d| d.str().map(str::to_string))
                .transpose()?
                .unwrap_or_else(|| label.clone()),
            label,
            n,
            absorbance,
        });
    }
    Ok(ReferenceSet {
        header,
        grid,
        levels,
    })
}
