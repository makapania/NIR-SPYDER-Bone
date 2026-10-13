"""Step 1: read an ASD file into reflectance, with the header fields the pipeline needs (DECISIONS 43, 87).

Three formats share the 484-byte header (offsets in bytes):
  0 magic (3 chars)   160 'when' (struct tm, 9 x int16: sec min hour mday mon year-1900 wday yday isdst)
  178 program_version (u8)   179 file_version (u8)   181 dc_corr (u8)   182 dc_time (i32, time_t UTC)
  186 data_type (u8; 0 raw, 1 reflectance)  187 ref_time (i32, time_t UTC)   191 ch1_wavel (f32)   195 wavel_step (f32)
  199 data_format (u8; 0 f32, 1 i32, 2 f64)   204 channels (u16)   390 integration time (u32, ms)
  400 instrument serial (u16)   421 flag bytes (4)   425 dc_count, ref_count, sample_count (3 x u16)
  436 swir1_gain, swir2_gain (u16)   444 splice1, splice2 wavelengths (2 x f32)
'as8' (LabSpec 4): raw sample DN (f64); reference header: i16 flag (-1), f64 reference time and f64 spectrum time
  (OLE automation dates, local), u16 length + description; channels x f64 white-reference DN; trailer.
'as7' (Trek-type handheld; layout verified on one unit): raw sample DN (f64); reference header: u8 flag (1), f64
  reference time at +1, f64 spectrum time at +9, u16 description length at +17, one byte; the white-reference DN
  (f64) starts at +20 + length; trailer (not parsed).
'ASD' (legacy v1, FieldSpec 3 '.NNN' and ViewSpec '.sco'): reflectance stored directly (data type 1, f32 or f64);
  no white reference, no raw DN.
R = sample DN / white-reference DN for raw files (PLAN Step 1); the stored reflectance for legacy files.
Detector joins (DECISIONS 87): the header joins when plausible (one per JOIN_WINDOWS_NM window), else the largest clear
white-reference DN jump in each window, else 1000/1800 ('default').
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


JOIN_WINDOWS_NM = ((950.0, 1050.0), (1700.0, 1900.0))
DEFAULT_JOINS_NM = (1000.0, 1800.0)
JOIN_JUMP_CLEAR = 3.0     # a detected join's DN jump must be >= 3 x the median jump in its window
JOIN_JUMP_MIN = 0.01      # ... and at least 1% (a window of mostly flat steps has a median of 0)
FORMATS = {"as8": "as8", "as7": "as7", "ASD": "legacy"}


def _joins_plausible(j):
    """Each join finite, on the 1-nm grid (a whole number) and inside its window."""
    return len(j) == 2 and all(np.isfinite(x) and float(x).is_integer() and lo <= x <= hi
                               for x, (lo, hi) in zip(j, JOIN_WINDOWS_NM))


def detect_joins(wl, ref):
    """The last channel before the largest clear relative white-reference DN jump in each join window, or None.
    Candidate steps k -> k+1 have both channels inside the window."""
    out = []
    for lo, hi in JOIN_WINDOWS_NM:
        k = np.flatnonzero((wl[:-1] >= lo) & (wl[1:] <= hi))
        if len(k) < 3:
            return None
        a, b = ref[k], ref[k + 1]
        with np.errstate(divide="ignore", invalid="ignore"):
            jump = np.abs(b - a) / np.abs(a)
        ok = np.isfinite(jump)
        if ok.sum() < 3:
            return None
        jj = np.where(ok, jump, -np.inf)
        i = int(np.argmax(jj))
        if not (jj[i] >= JOIN_JUMP_CLEAR * float(np.median(jump[ok])) and jj[i] >= JOIN_JUMP_MIN):
            return None
        out.append(float(wl[k[i]]))
    return out


def resolve_joins(h, wl, ref):
    """(joins, source): header if plausible, else detected from the white reference, else the default."""
    j = h["splices_header_nm"]
    if _joins_plausible(j):
        return [float(x) for x in j], "header"
    if ref is not None:
        d = detect_joins(wl, ref)
        if d is not None:
            return d, "reference_dn"
    return [float(x) for x in DEFAULT_JOINS_NM], "default"


def read(path):
    """path: a file path, or the file's bytes. Returns (h, wl, dn, ref); for a legacy reflectance file dn and ref are
    None and the stored reflectance is h['stored_R']."""
    b = bytes(path) if isinstance(path, (bytes, bytearray)) else open(path, "rb").read()
    if len(b) < 484:
        raise AsdError("unsupported: file shorter than the header")
    h = {"bytes_sha256": hashlib.sha256(b).hexdigest(), "n_bytes": len(b)}
    h["magic"] = b[0:3].decode("latin1")
    h["format"] = FORMATS.get(h["magic"])
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
    h["splices_header_nm"] = [float(x) for x in struct.unpack_from("<ff", b, 444)]
    n = h["channels"]
    if h["data_format"] not in DT:
        raise AsdError("unsupported: unknown data format")
    dt, sz = DT[h["data_format"]]
    off = 484
    if len(b) < off + n * sz:
        raise AsdError("unsupported: truncated sample block")
    dn = np.frombuffer(b, dtype=dt, count=n, offset=off).astype(float)
    off += n * sz
    wl = h["ch1_wavel"] + h["wavel_step"] * np.arange(n)
    ref = None
    h["ole_ref_time"] = h["ole_spec_time"] = None
    if h["format"] == "legacy":
        h["stored_R"] = dn
        dn = None
    elif h["format"] == "as7":
        if len(b) >= off + 20:
            h["ref_flag"] = b[off]
            h["ole_ref_time"], h["ole_spec_time"] = struct.unpack_from("<dd", b, off + 1)
            L = struct.unpack_from("<H", b, off + 17)[0]
            o2 = off + 20 + L
            if len(b) >= o2 + n * 8:
                ref = np.frombuffer(b, dtype="<f8", count=n, offset=o2).astype(float)
    elif len(b) >= off + 20:
        h["ref_flag"] = struct.unpack_from("<h", b, off)[0]
        h["ole_ref_time"], h["ole_spec_time"] = struct.unpack_from("<dd", b, off + 2)
        L = struct.unpack_from("<H", b, off + 18)[0]
        o2 = off + 20 + L
        if len(b) >= o2 + n * 8:
            ref = np.frombuffer(b, dtype="<f8", count=n, offset=o2).astype(float)
    h["splices_nm"], h["joins_source"] = resolve_joins(h, wl, ref)
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


def joins_accepted(joins, matrix):
    """Each join inside its window (DECISIONS 87; spectrum input asserts its joins, files resolve them)."""
    w = matrix["join_windows_nm"]
    j = sorted(float(x) for x in joins)
    return len(j) == len(w) and all(lo <= x <= hi for x, (lo, hi) in zip(j, w))


def acceptance(h, wl, dn, ref, matrix):
    """Step 1 supported-input matrix (check.acquisition parameters 'input_matrix'). Returns None if accepted, else
    the 'unsupported: <reason>' text (B1)."""
    if h["magic"] not in matrix["file_magic"]:
        return f"unsupported: file version {h['magic']!r}"
    if h["data_type"] != matrix["data_type_by_magic"][h["magic"]]:
        return ("unsupported: not a raw (DN + white reference) file" if h["format"] != "legacy"
                else "unsupported: not a reflectance file")
    # data formats as the Rust reader (float64 for raw files; float32 or float64 for legacy reflectance)
    if h["format"] == "legacy" and h["data_format"] not in (0, 2):
        return f"unsupported: data format {h['data_format']} (legacy reflectance files must be float32 = 0 or float64 = 2)"
    if h["format"] in ("as8", "as7") and h["data_format"] != 2:
        return f"unsupported: data format {h['data_format']} (only float64 = 2 is supported)"
    if h["magic"] in matrix["require_dark_corrected_magic"] and not h["dc_corr"]:
        return "unsupported: dark correction flag not set"
    g = matrix["grid"]
    if h["channels"] != g["n"] or abs(h["ch1_wavel"] - g["start_nm"]) > 1e-6 or abs(h["wavel_step"] - g["step_nm"]) > 1e-6:
        return "unsupported: wavelength grid"
    lo, hi = matrix["reference_range_nm"]
    k = (wl >= lo) & (wl <= hi)
    if h["format"] == "legacy":
        if not np.all(np.isfinite(h["stored_R"])):
            return "unsupported: non-finite reflectance values"
    else:
        if ref is None:
            return "unsupported: no white-reference block"
        if not np.all(np.isfinite(ref[k])) or np.any(ref[k] <= 0):
            return "unsupported: white reference not finite and positive"
        if not np.all(np.isfinite(dn)):
            return "unsupported: non-finite sample values"
    if not joins_accepted(h["splices_nm"], matrix):
        return f"unsupported: detector joins {h['splices_nm']}"
    return None


def reflectance_of(h, dn, ref):
    """The scan's reflectance: stored (legacy) or sample DN / white-reference DN."""
    return np.asarray(h["stored_R"], float) if h["format"] == "legacy" else reflectance(dn, ref)


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


def write_as7(R, ref, serial, ole_ref, ole_spec, ref_time_t, joins=(0.0, 0.0), integration_ms=17, counts=(100, 100, 50)):
    """Build an as7 file (bytes) in the Trek-type layout (u8 reference flag; dark flag 0; trailer omitted). Goldens only."""
    n = len(R)
    h = bytearray(484)
    h[0:3] = b"as7"
    h[178] = 16; h[179] = 16; h[181] = 0; h[186] = 0; h[199] = 2
    struct.pack_into("<i", h, 182, int(ref_time_t)); struct.pack_into("<i", h, 187, int(ref_time_t))
    struct.pack_into("<ff", h, 191, 350.0, 1.0)
    struct.pack_into("<H", h, 204, n)
    struct.pack_into("<I", h, 390, int(integration_ms))
    struct.pack_into("<H", h, 400, int(serial))
    struct.pack_into("<HHH", h, 425, *counts)
    struct.pack_into("<ff", h, 444, float(joins[0]), float(joins[1]))
    dn = np.asarray(R, float) * np.asarray(ref, float)
    tail = struct.pack("<Bdd", 1, float(ole_ref), float(ole_spec)) + struct.pack("<H", 0) + bytes(1)
    return bytes(h) + dn.astype("<f8").tobytes() + tail + np.asarray(ref, float).astype("<f8").tobytes()


def write_legacy(R, serial, ref_time_t, joins=(1000.0, 1800.0), integration_ms=34, counts=(60, 60, 100),
                 splice_corrected=False):
    """Build a legacy v1 reflectance file (magic 'ASD', data type 1, f32), as a FieldSpec 3 '.NNN' (or, with
    splice_corrected, its ViewSpec '.sco': flag byte 421 = 0x04). Goldens only."""
    n = len(R)
    h = bytearray(484)
    h[0:3] = b"ASD"
    h[178] = 100; h[179] = 16 if splice_corrected else 68; h[181] = 1; h[186] = 1; h[199] = 0
    struct.pack_into("<i", h, 182, int(ref_time_t)); struct.pack_into("<i", h, 187, int(ref_time_t))
    struct.pack_into("<ff", h, 191, 350.0, 1.0)
    struct.pack_into("<H", h, 204, n)
    struct.pack_into("<I", h, 390, int(integration_ms))
    struct.pack_into("<H", h, 400, int(serial))
    h[421] = 4 if splice_corrected else 0
    struct.pack_into("<HHH", h, 425, *counts)
    struct.pack_into("<ff", h, 444, float(joins[0]), float(joins[1]))
    return bytes(h) + np.asarray(R, float).astype("<f4").tobytes()
