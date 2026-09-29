export function nextCopyName(path: string, taken: Iterable<string>): string {
  const slash = path.lastIndexOf('/');
  const dir = slash === -1 ? '' : path.slice(0, slash + 1);
  const name = path.slice(slash + 1);
  const dot = name.lastIndexOf('.');
  const stem = dot === -1 ? name : name.slice(0, dot);
  const ext = dot === -1 ? '' : name.slice(dot);

  const numbered = stem.match(/^(.*?)-(\d+)$/);
  const base = numbered ? numbered[1] : stem;
  const from = numbered ? Number(numbered[2]) + 1 : 2;

  const used = new Set(taken);
  for (let n = from; ; n++) {
    const candidate = `${dir}${base}-${n}${ext}`;
    if (!used.has(candidate)) return candidate;
  }
}

export function copiedNote(name: string, from: string, dirty: boolean): string {
  const original = from.split('/').pop() ?? from;
  return dirty
    ? `${name} — a copy of ${original} as it is on disk, without the unsaved edits`
    : `${name} — a copy of ${original}`;
}
