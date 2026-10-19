// The window's three columns (App.svelte): the scans list, the main column and the details panel. Their default widths
// follow the window (laptops first, DECISIONS 67); the scans list can be widened by the viewer (Matt, 2026-10-18: long
// file names and the row marks need room), within bounds that keep the main column usable down to the 1100 px window.

/** The scans list's and the details panel's default widths at this window width (App.svelte's breakpoints). */
export function columnDefaults(viewport: number): { list: number; details: number } {
  if (viewport <= 1300) return { list: 216, details: 290 };
  if (viewport <= 1366) return { list: 236, details: 312 };
  return { list: 272, details: 344 };
}

/** The narrowest list the rows are designed for (a long reading and a flag always fit). */
export const LIST_MIN = 216;
/** The widest list worth having. */
export const LIST_MAX = 560;
/** The main column keeps at least this much (the band close-ups' row and the spectrum need it). */
export const MAIN_MIN = 540;

/** The list's allowed widths at this window width. */
export function listWidthBounds(viewport: number): { min: number; max: number } {
  const room = viewport - columnDefaults(viewport).details - MAIN_MIN;
  return { min: LIST_MIN, max: Math.max(LIST_MIN, Math.min(LIST_MAX, room)) };
}

/** A requested list width, kept within its bounds (whole pixels). */
export function clampListWidth(w: number, viewport: number): number {
  const { min, max } = listWidthBounds(viewport);
  return Math.round(Math.min(max, Math.max(min, Number.isFinite(w) ? w : min)));
}

/** A stored width (localStorage text) as a number, or null when absent or not a usable number. */
export function parseListWidth(v: string | null): number | null {
  if (v == null || v === '') return null;
  const n = Number(v);
  return Number.isFinite(n) && n > 0 ? n : null;
}
