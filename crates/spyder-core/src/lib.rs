//! SPYDER Bone numerical core (PLAN v3.2 section 2; Phase 1).
//!
//! * [`read`]: the ASD `as8` reader for the supported-input matrix (PLAN section 3 Step 1).
//! * [`n2`]: the canonical noise measure N2 (port of `phase0a/common/n2.py`).
//! * [`qc`]: acquisition checks B1-B8 on the as-measured scan (Step 2).
//! * [`grid`]: grid eligibility (Step 1).
//! * [`preprocess`]: the v1 operator subset, engine 1.1 (section 4).
//! * [`plugins`]: plug-in discovery, validation, precedence, pins, sidecars and the golden runner (Phase 2).
//! * [`model`]: linear models and the CONS3 consensus; [`transfer`]: high-res -> standard-res transfers.
//!
//! No UI or Tauri dependency; all maths in float64; nothing here panics on bad input.

#![forbid(unsafe_code)]

pub mod grid;
pub mod model;
pub mod n2;
pub mod numsum;
pub mod pipeline;
pub mod plugins;
pub mod predict;
pub mod preprocess;
pub mod pyfmt;
pub mod qc;
pub mod read;
pub mod scan;
pub mod status;
pub mod timefmt;
pub mod transfer;

pub use read::{read_bytes, read_file, ReadError};
pub use scan::{Scan, ScanKind};
pub use status::Status;
