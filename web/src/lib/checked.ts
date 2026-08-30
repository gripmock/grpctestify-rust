import { count } from 'luvo/data/plural';
export interface CheckedFile {
  path: string;
  errors: number;
  warnings: number;
  first?: string;
}

export interface CheckMark {
  kind: 'error' | 'warn';
  label: string;
  title: string;
}

export function checkMark(checked: CheckedFile | undefined): CheckMark | null {
  if (!checked) return null;
  const { errors, warnings, first } = checked;
  if (errors === 0 && warnings === 0) return null;
  const kind = errors > 0 ? 'error' : 'warn';
  const counted = errors > 0
    ? `${count(errors, 'error')}`
    : `${count(warnings, 'warning')}`;
  return {
    kind,
    label: String(errors > 0 ? errors : warnings),
    title: first ? `${counted} — ${first}` : counted,
  };
}

export function checkSummary(
  files: CheckedFile[],
  checked: number,
  truncated: boolean,
): string {
  if (files.length === 0) {
    return `${count(checked, 'file')} checked — nothing to report`;
  }
  const errors = files.reduce((n, f) => n + f.errors, 0);
  const warnings = files.reduce((n, f) => n + f.warnings, 0);
  const parts = [
    errors > 0 ? `${count(errors, 'error')}` : null,
    warnings > 0 ? `${count(warnings, 'warning')}` : null,
  ].filter(Boolean);
  return `${parts.join(' · ')} in ${files.length} of ${count(checked, 'file')}`
    + (truncated ? ' — the first 500' : '');
}

export function mergeChecked(
  previous: Record<string, CheckedFile>,
  asked: string[],
  answered: CheckedFile[],
): Record<string, CheckedFile> {
  const next = { ...previous };
  for (const path of asked) delete next[path];
  for (const file of answered) next[file.path] = file;
  return next;
}

export function checkedAfterMove(
  previous: Record<string, CheckedFile>,
  from: string,
  to: string | null,
): Record<string, CheckedFile> {
  const moved: Record<string, CheckedFile> = {};
  for (const [path, file] of Object.entries(previous)) {
    const inside = path === from || path.startsWith(`${from}/`);
    if (!inside) { moved[path] = file; continue; }
    if (to === null) continue;
    const renamed = to + path.slice(from.length);
    moved[renamed] = { ...file, path: renamed };
  }
  return moved;
}

export function rollUpChecks(
  paths: string[],
  checked: Record<string, CheckedFile>,
): { files: number; errors: number; warnings: number } {
  let files = 0;
  let errors = 0;
  let warnings = 0;
  for (const path of paths) {
    const file = checked[path];
    if (!file) continue;
    files += 1;
    errors += file.errors;
    warnings += file.warnings;
  }
  return { files, errors, warnings };
}
