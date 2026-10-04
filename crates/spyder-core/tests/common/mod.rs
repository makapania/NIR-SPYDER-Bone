//! Synthetic ASD `as8` builder for public tests (no measured spectra). Layout per 03 section 3.2.
#![allow(dead_code)]

pub const N: usize = 2151;

pub fn wl() -> Vec<f64> {
    (0..N).map(|i| 350.0 + i as f64).collect()
}

/// A smooth white-reference DN shape with steps at 1000/1800 nm (as the real reference shows).
pub fn reference_dn() -> Vec<f64> {
    wl().iter()
        .map(|&w| {
            let base = 20_000.0 + 8_000.0 * (-(((w - 1300.0) / 600.0).powi(2))).exp();
            if w <= 1000.0 {
                base * 0.8
            } else if w <= 1800.0 {
                base
            } else {
                base * 0.6
            }
        })
        .collect()
}

/// A bone-like reflectance in 0.2-0.5 with small deterministic ripples.
pub fn bone_reflectance() -> Vec<f64> {
    wl().iter()
        .map(|&w| {
            0.35 + 0.08 * (w / 211.0).sin() - 0.05 * (-(((w - 1930.0) / 40.0).powi(2))).exp()
                + 2e-5 * (w * 1.7).sin()
        })
        .collect()
}

#[derive(Clone)]
pub struct AsdSpec {
    pub magic: [u8; 3],
    pub data_type: u8,
    pub data_format: u8,
    pub dark_corrected: u8,
    pub channels: u16,
    pub first_nm: f32,
    pub step_nm: f32,
    pub splices: [f32; 2],
    pub serial: u16,
    pub it_ms: u32,
    pub swir_gains: [u16; 2],
    pub swir_offsets: [u16; 2],
    pub averages: [u16; 3],
    pub ref_flag: u16,
    pub reference_ole: f64,
    pub spectrum_ole: f64,
    pub reference_time_t: u32,
    pub description: Vec<u8>,
    pub sample: Vec<f64>,
    pub reference: Vec<f64>,
    pub trailer: Vec<u8>,
}

impl Default for AsdSpec {
    fn default() -> Self {
        let r = reference_dn();
        let s: Vec<f64> = r
            .iter()
            .zip(bone_reflectance())
            .map(|(a, b)| a * b)
            .collect();
        // 2026-09-08 13:41:48 local reference, sample 11 min 26 s later; local = UTC - 4 h
        let ref_ole = 46273.0 + (13.0 * 3600.0 + 41.0 * 60.0 + 48.0) / 86400.0;
        let ref_unix_local = (ref_ole - 25569.0) * 86400.0;
        AsdSpec {
            magic: *b"as8",
            data_type: 0,
            data_format: 2,
            dark_corrected: 1,
            channels: N as u16,
            first_nm: 350.0,
            step_nm: 1.0,
            splices: [1000.0, 1800.0],
            serial: 12345,
            it_ms: 136,
            swir_gains: [16, 21],
            swir_offsets: [2050, 2075],
            averages: [100, 50, 50],
            ref_flag: 0xFFFF,
            reference_ole: ref_ole,
            spectrum_ole: ref_ole + 686.0 / 86400.0,
            reference_time_t: (ref_unix_local + 4.0 * 3600.0).round() as u32,
            description: Vec::new(),
            sample: s,
            reference: r,
            trailer: vec![0u8; 212],
        }
    }
}

impl AsdSpec {
    pub fn build(&self) -> Vec<u8> {
        let mut h = vec![0u8; 484];
        h[0..3].copy_from_slice(&self.magic);
        // struct tm of the sample time (local): 2026-09-08 13:53:14, isdst 1
        let tm: [i16; 9] = [14, 53, 13, 8, 8, 126, 2, 250, 1];
        for (k, v) in tm.iter().enumerate() {
            h[160 + 2 * k..162 + 2 * k].copy_from_slice(&v.to_le_bytes());
        }
        h[178] = 102;
        h[179] = 0x80;
        h[181] = self.dark_corrected;
        h[182..186].copy_from_slice(&self.reference_time_t.to_le_bytes());
        h[186] = self.data_type;
        h[187..191].copy_from_slice(&self.reference_time_t.to_le_bytes());
        h[191..195].copy_from_slice(&self.first_nm.to_le_bytes());
        h[195..199].copy_from_slice(&self.step_nm.to_le_bytes());
        h[199] = self.data_format;
        h[204..206].copy_from_slice(&self.channels.to_le_bytes());
        h[390..394].copy_from_slice(&self.it_ms.to_le_bytes());
        h[398..400].copy_from_slice(&1u16.to_le_bytes());
        h[400..402].copy_from_slice(&self.serial.to_le_bytes());
        h[418..420].copy_from_slice(&16u16.to_le_bytes());
        for (k, a) in self.averages.iter().enumerate() {
            h[425 + 2 * k..427 + 2 * k].copy_from_slice(&a.to_le_bytes());
        }
        h[431] = 4;
        h[436..438].copy_from_slice(&self.swir_gains[0].to_le_bytes());
        h[438..440].copy_from_slice(&self.swir_gains[1].to_le_bytes());
        h[440..442].copy_from_slice(&self.swir_offsets[0].to_le_bytes());
        h[442..444].copy_from_slice(&self.swir_offsets[1].to_le_bytes());
        h[444..448].copy_from_slice(&self.splices[0].to_le_bytes());
        h[448..452].copy_from_slice(&self.splices[1].to_le_bytes());
        let mut out = h;
        for v in &self.sample {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.ref_flag.to_le_bytes());
        out.extend_from_slice(&self.reference_ole.to_le_bytes());
        out.extend_from_slice(&self.spectrum_ole.to_le_bytes());
        out.extend_from_slice(&(self.description.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.description);
        for v in &self.reference {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.trailer);
        out
    }

    /// Set the sample block so that R = r exactly where possible (sample = r * reference).
    pub fn with_reflectance(mut self, r: &[f64]) -> Self {
        self.sample = self.reference.iter().zip(r).map(|(a, b)| a * b).collect();
        self
    }
}

pub fn idx(nm: f64) -> usize {
    (nm - 350.0) as usize
}
