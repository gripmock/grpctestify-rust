export function relativeToFile(filePath: string | null, target: string): string {
  if (!filePath || target.trim() === '') return target;
  if (target.startsWith('/') || /^[A-Za-z]:[\\/]/.test(target)) return target;
  const from = filePath.split('/').slice(0, -1).filter(Boolean);
  const to = target.split('/').filter(Boolean);
  if (from.length === 0) return to.join('/');
  let shared = 0;
  while (shared < from.length && shared < to.length - 1 && from[shared] === to[shared]) shared += 1;
  const up = Array.from({ length: from.length - shared }, () => '..');
  return [...up, ...to.slice(shared)].join('/');
}

export function fromFileRelative(filePath: string | null, spelled: string): string {
  if (!filePath || spelled.trim() === '') return spelled;
  if (spelled.startsWith('/') || /^[A-Za-z]:[\\/]/.test(spelled)) return spelled;
  const parts = filePath.split('/').slice(0, -1).filter(Boolean);
  for (const piece of spelled.split('/')) {
    if (piece === '' || piece === '.') continue;
    if (piece === '..') parts.pop();
    else parts.push(piece);
  }
  return parts.join('/');
}
