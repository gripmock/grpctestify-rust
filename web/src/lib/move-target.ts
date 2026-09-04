export function moveRefusal(
  from: string,
  to: string,
  paths: Iterable<string>,
): string | null {
  const target = clean(to);
  if (target === '' || target === from) return null;
  return outsideNote(target) ?? takenNote(target, paths, 'file');
}

export function createRefusal(
  target: string,
  paths: Iterable<string>,
  kind: 'file' | 'folder',
): string | null {
  const path = clean(target);
  if (path === '') return null;
  return outsideNote(path, kind) ?? takenNote(path, paths, kind);
}

function clean(raw: string): string {
  return raw.trim().replace(/^\.\//, '');
}

function outsideNote(target: string, kind: 'file' | 'folder' = 'file'): string | null {
  if (target.startsWith('/')) {
    return 'A path inside the collections folder — it cannot start with `/`.';
  }
  if (target.split('/').some(part => part === '..')) {
    return 'A path inside the collections folder — `..` leaves it.';
  }
  if (kind === 'file' && target.endsWith('/')) {
    return `${target} names a folder — the file needs a name of its own.`;
  }
  return null;
}

function takenNote(target: string, paths: Iterable<string>, kind: 'file' | 'folder'): string | null {
  const name = target.replace(/\/$/, '');
  for (const path of paths) {
    if (path === name) {
      return kind === 'folder'
        ? `${name} is already a file — pick another name.`
        : `${name} already exists — pick another name.`;
    }
    if (path.startsWith(`${name}/`)) {
      return kind === 'folder'
        ? `${name} is already here — pick another name.`
        : `${name} is a folder — pick another name.`;
    }
  }
  return null;
}
