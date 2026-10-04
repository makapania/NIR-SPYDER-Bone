"""Step 1: read an ASD LabSpec 4 .asd file (format as8) into reflectance, with the header fields the pipeline needs.

Header layout (ASD file format v8, 484-byte header; offsets in bytes):
  0 magic (3 chars, 'as8')   160 'when' (struct tm, 9 x int16: sec min hour mday mon year-1900 wday yday isdst)
  178 program_version (u8)   179 file_version (u8)   181 dc_corr (u8)   182 dc_time (i32, time_t UTC)
  186 data_type (u8; 0 raw)  187 ref_time (i32, time_t UTC)   191 ch1_wavel (f32)   195 wavel_step (f32)
  199 data_format (u8; 0 f32, 1 i32, 2 f64)   204 channels (u16)   390 integration time (u32, ms)
  400 instrument serial (u16)   425 dc_count, ref_count, sample_count (3 x u16)   436 swir1_gain, swir2_gain (u16)
  444 splice1, splice2 wavelengths (2 x f32)
After the header: the sample block (channels x dtype); then the reference block: i16 flag, f64 reference time
(OLE automation date, local), f64 spectrum time (OLE, local), u16 length + description, channels x f64 white
reference DN.
R = sample DN / white-reference DN (PLAN Step 1).
"""
from __future__ import annotations

import datetime as _dt
import hashlib
import struct

import numpy as np

DT = {0: ("<f4", 4), 1: ("<i4", 4), 2: ("<f8", 8)}
OLE_EPOCH = _dt.datetime(1899, 12, 30)


class AsdError(ValueError):
    pass


def read(path):
    """path: a file path, or the file's bytes."""
    b = bytes(path) if isinstance(path, (bytes, bytearray)) else open(path, "rb").read()
    if len(b) < 484:
        raise AsdError("unsupported: file shorter than the header")
    h = {"bytes_sha256": hashlib.sha256(b).hexdigest(), "n_bytes": len(b)}
    h["magic"] = b[0:3].decode("latin1")
    h["program_version"] = b[178]
    h["file_version"] = b[179]
    h["dc_corr"] = b[181]
    h["dc_time"] = struct.unpack_from("<i", b, 182)[0]
    h["data_type"] = b[186]
    h["ref_time"] = struct.unpack_from("<i", b, 187)[0]
    h["ch1_wavel"] = struct.unpack_from("<f", b, 191)[0]
    h["wavel_step"] = struct.unpack_from("<f", b, 195)[0]
    h["data_format"] = b[199]
    h["channels"] = struct.unpack_from("<H", b, 204)[0]
    h["integration_time_ms"] = struct.unpack_from("<I", b, 390)[0]
    h["serial"] = struct.unpack_from("<H", b, 400)[0]
    h["dc_count"], h["ref_count"], h["sample_count"] = struct.unpack_from("<HHH", b, 425)
    h["swir1_gain"], h["swir2_gain"] = struct.unpack_from("<HH", b, 436)
    h["splices_nm"] = [float(x) for x in struct.unpack_from("<ff", b, 444)]
    n = h["channels"]
    if h["data_format"] not in DT:
        raise AsdError("unsupported: unknown data format")
    dt, sz = DT[h["data_format"]]
    off = 484
    if len(b) < off + n * sz:
        raise AsdError("unsupported: truncated sample block")
    dn = np.frombuffer(b, dtype=dt, count=n, offset=off).astype(float)
    off += n * sz
    ref = None
    h["ole_ref_time"] = h["ole_spec_time"] = None
    if len(b) >= off + 20:
        h["ref_flag"] = struct.unpack_from("<h", b, off)[0]
        h["ole_ref_time"], h["ole_spec_time"] = struct.unpack_from("<dd", b, off + 2)
        L = struct.unpack_from("<H", b, off + 18)[0]
        o2 = off + 20 + L
        if len(b) >= o2 + n * 8:
            ref = np.frombuffer(b, dtype="<f8", count=n, offset=o2).astype(float)
    wl = h["ch1_wavel"] + h["wavel_step"] * np.arange(n)
    return h, wl, dn, ref


def ole_to_iso(ole):
    if ole is None or not np.isfinite(ole):
        return None
    return (OLE_EPOCH + _dt.timedelta(days=float(ole))).isoformat(timespec="seconds")


def utc_offset_minutes(h):
    """Local time (OLE reference time) minus UTC (time_t reference time), rounded to 15 minutes; None if unknown."""
    if h.get("ole_ref_time") is None or not h.get("ref_time"):
        return None
    local = OLE_EPOCH + _dt.timedelta(days=float(h["ole_ref_time"]))
    utc = _dt.datetime(1970, 1, 1) + _dt.timedelta(seconds=int(h["ref_time"]))
    m = (local - utc).total_seconds() / 60.0
    return int(round(m / 15.0) * 15)


def acceptance(h, wl, dn, ref, matrix):
    """Step 1 supported-input matrix (check.acquisition parameters 'input_matrix'). Returns None if accepted, else
    the 'unsupported: <reason>' text (B1)."""
    if h["magic"] not in matrix["file_magic"]:
        return f"unsupported: file version {h['magic']!r}"
    if h["data_type"] != matrix["data_type_raw"]:
        return "unsupported: not a raw (DN + white reference) file"
    if matrix.get("require_dark_corrected") and not h["dc_corr"]:
        return "unsupported: dark correction flag not set"
    g = matrix["grid"]
    if h["channels"] != g["n"] or abs(h["ch1_wavel"] - g["start_nm"]) > 1e-6 or abs(h["wavel_step"] - g["step_nm"]) > 1e-6:
        return "unsupported: wavelength grid"
    if ref is None:
        return "unsupported: no white-reference block"
    lo, hi = matrix["reference_range_nm"]
    k = (wl >= lo) & (wl <= hi)
    if not np.all(np.isfinite(ref[k])) or np.any(ref[k] <= 0):
        return "unsupported: white reference not finite and positive"
    if not np.all(np.isfinite(dn)):
        return "unsupported: non-finite sample values"
    joins = matrix["accepted_joins_nm"]
    if any(abs(a - b) > 1e-3 for a, b in zip(sorted(h["splices_nm"]), joins)):
        return f"unsupported: detector joins {h['splices_nm']} (only {joins} are supported)"
    return None


def reflectance(dn, ref):
    with np.errstate(divide="ignore", invalid="ignore"):
        return dn / ref


def write_as8(R, ref, serial, ole_ref, ole_spec, ref_time_t, joins=(1000.0, 1800.0), integration_ms=34,
              counts=(100, 50, 50)):
    """Build an as8 file (bytes) from reflectance and a white-reference DN curve: sample DN = R x ref, float64
    blocks. For golden files only (synthetic input to the reader); every field the reader uses is set."""
    n = len(R)
    h = bytearray(484)
    h[0:3] = b"as8"
    h[178] = 6; h[179] = 128; h[181] = 1; h[186] = 0; h[199] = 2
    struct.pack_into("<i", h, 182, int(ref_time_t)); struct.pack_into("<i", h, 187, int(ref_time_t))
    struct.pack_into("<ff", h, 191, 350.0, 1.0)
    struct.pack_into("<H", h, 204, n)
    struct.pack_into("<I", h, 390, int(integration_ms))
    struct.pack_into("<H", h, 400, int(serial))
    struct.pack_into("<HHH", h, 425, *counts)
    struct.pack_into("<ff", h, 444, float(joins[0]), float(joins[1]))
    dn = np.asarray(R, float) * np.asarray(ref, float)
    tail = struct.pack("<hdd", -1, float(ole_ref), float(ole_spec)) + struct.pack("<H", 0)
    return bytes(h) + dn.astype("<f8").tobytes() + tail + np.asarray(ref, float).astype("<f8").tobytes()
