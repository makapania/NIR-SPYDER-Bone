//! Assessment status, exported separately from every result (PLAN section 3 preamble; Codex MEDIUM-12),
//! so that a skipped check never reads as "not detected".

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    /// The check ran; its result is meaningful.
    Assessed,
    /// The check could not run on this scan (grid, noise, reference save, missing parameters...).
    NotAssessed { reason: String },
    /// The check was deliberately switched off by another check (e.g. B6b gates the contaminant signs).
    Gated { reason: String },
}

impl Status {
    pub fn not_assessed(reason: impl Into<String>) -> Self {
        Status::NotAssessed {
            reason: reason.into(),
        }
    }

    pub fn gated(reason: impl Into<String>) -> Self {
        Status::Gated {
            reason: reason.into(),
        }
    }

    pub fn is_assessed(&self) -> bool {
        matches!(self, Status::Assessed)
    }

    /// "assessed", "not assessed: <reason>", "gated: <reason>".
    pub fn describe(&self) -> String {
        match self {
            Status::Assessed => "assessed".to_string(),
            Status::NotAssessed { reason } => format!("not assessed: {reason}"),
            Status::Gated { reason } => format!("gated: {reason}"),
        }
    }
}
