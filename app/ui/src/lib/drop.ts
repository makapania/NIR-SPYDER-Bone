/** What a drag-and-drop onto the window opens: the .asd files among the dropped paths, else a single dropped folder
 *  (opened like Open folder: its .asd files, not sub-folders). Anything else is explained, never silently ignored. */
export type DropAction = { kind: 'files'; paths: string[] } | { kind: 'folder'; path: string } | { kind: 'none'; message: string };

export function classifyDrop(paths: string[]): DropAction {
  const asd = paths.filter((p) => /\.asd$/i.test(p));
  if (asd.length) return { kind: 'files', paths: asd };
  if (paths.length === 1) return { kind: 'folder', path: paths[0] };
  return { kind: 'none', message: 'Drop .asd files or one folder of them.' };
}
