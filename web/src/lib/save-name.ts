import { nextCopyName } from './duplicate-name';

export interface SeededName {
  name: string;
  taken: string | null;
}

export function seedSaveName(input: {
  base: string;
  ext: string;
  folder: string;
  paths: Iterable<string>;
}): SeededName {
  const base = input.base.trim();
  if (base === '') return { name: '', taken: null };
  const dir = input.folder === '' ? '' : `${input.folder}/`;
  const path = `${dir}${base}.${input.ext}`;
  const paths = new Set(input.paths);
  if (!paths.has(path)) return { name: base, taken: null };
  const free = nextCopyName(path, paths);
  const stem = free.slice(dir.length).replace(new RegExp(`\\.${input.ext}$`), '');
  return { name: stem, taken: path };
}
