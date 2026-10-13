/** What a drag-and-drop onto the window opens: the ASD scan files among the dropped paths (.asd, ViewSpec .sco,
 *  FieldSpec 3 .000), else a single dropped folder (opened like Open folder: its scan files, not sub-folders).
 *  Anything else is explained, never silently ignored. */
export type DropAction = { kind: 'files'; paths: string[] } | { kind: 'folder'; path: string } | { kind: 'none'; message: string };

/** An ASD scan file name: .asd, .sco or a three-digit extension (FieldSpec 3), any case (DECISIONS 87-88). */
export function isScanFile(p: string): boolean {
  return /[^/\\]\.(asd|sco|\d{3})$/i.test(p);
}

export function classifyDrop(paths: string[]): DropAction {
  const asd = paths.filter((p) => isScanFile(p));
  if (asd.length) return { kind: 'files', paths: asd };
  if (paths.length === 1) return { kind: 'folder', path: paths[0] };
  return { kind: 'none', message: 'Drop ASD scan files (.asd, .sco, .000) or one folder of them.' };
}
