import type { GctfDiagnostic } from './types';

export const ERROR = 1;
export const WARNING = 2;
export const INFO = 3;

export type ProblemCounts = { errors: number; warnings: number; infos: number };

export function countBySeverity(problems: GctfDiagnostic[]): ProblemCounts {
  const counts: ProblemCounts = { errors: 0, warnings: 0, infos: 0 };
  for (const p of problems) {
    const severity = p.severity ?? ERROR;
    if (severity === ERROR) counts.errors++;
    else if (severity === WARNING) counts.warnings++;
    else counts.infos++;
  }
  return counts;
}

export function problemCensus(counts: ProblemCounts): string {
  const notes = counts.warnings + counts.infos;
  if (counts.errors === 0) {
    return `${notes} ${notes === 1 ? 'note' : 'notes'}`;
  }
  const errors = `${counts.errors} ${counts.errors === 1 ? 'error' : 'errors'}`;
  const said = notes === 0 ? errors : `${errors} · ${notes} ${notes === 1 ? 'note' : 'notes'}`;
  return `${said} — this file would not pass check`;
}

export function sortProblems(problems: GctfDiagnostic[]): GctfDiagnostic[] {
  return [...problems].sort((a, b) => {
    const sa = a.severity ?? ERROR;
    const sb = b.severity ?? ERROR;
    if (sa !== sb) return sa - sb;
    if (a.range.start.line !== b.range.start.line) return a.range.start.line - b.range.start.line;
    return a.range.start.character - b.range.start.character;
  });
}

export function severityLabel(severity: number | undefined): 'error' | 'warning' | 'info' {
  const s = severity ?? ERROR;
  if (s === ERROR) return 'error';
  if (s === WARNING) return 'warning';
  return 'info';
}

export function aboutWholeFile(p: GctfDiagnostic): boolean {
  return p.data?.scope === 'file';
}

export function problemKey(p: GctfDiagnostic): string {
  return `${p.code ?? ''}:${p.message}`;
}

export function blocksSave(problems: GctfDiagnostic[]): boolean {
  return countBySeverity(problems).errors > 0;
}

export function matchLine(diagnosed: string, line: number, target: string): number {
  const found = matchLineExact(diagnosed, line, target);
  if (found !== -1) return found;
  const to = target.split('\n');
  return Math.min(line, Math.max(0, to.length - 1));
}

export function matchLineExact(diagnosed: string, line: number, target: string): number {
  const from = diagnosed.split('\n');
  const to = target.split('\n');
  const needle = from[line];
  if (needle === undefined) return -1;
  if (to[line] === needle) return line;
  if (needle.trim() === '') return -1;

  let best = -1;
  for (let i = 0; i < to.length; i++) {
    if (to[i] !== needle) continue;
    if (best === -1 || Math.abs(i - line) < Math.abs(best - line)) best = i;
  }
  return best;
}

export interface ProblemMarker {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
  severity: number;
  message: string;
  code?: string;
}

export function problemMarkers(
  problems: GctfDiagnostic[],
  diagnosed: string | null,
  text: string,
): ProblemMarker[] {
  if (diagnosed === null) return [];
  const own = diagnosed === text;
  const lines = text.split('\n');
  const markers: ProblemMarker[] = [];
  for (const p of problems) {
    if (aboutWholeFile(p)) continue;
    const line = own ? p.range.start.line : matchLineExact(diagnosed, p.range.start.line, text);
    if (line === -1 || line >= lines.length) continue;
    const code = typeof p.code === 'string' || typeof p.code === 'number' ? String(p.code) : undefined;
    markers.push({
      startLine: line + 1,
      startColumn: own ? p.range.start.character + 1 : 1,
      endLine: own ? p.range.end.line + 1 : line + 1,
      endColumn: own ? p.range.end.character + 1 : lines[line].length + 1,
      severity: p.severity ?? ERROR,
      message: p.message,
      ...(code === undefined ? {} : { code }),
    });
  }
  return markers;
}

export function diagnosticsVoice(input: { path: string | null; dirty: boolean }): 'check' | 'editor' {
  return input.path !== null && !input.dirty ? 'check' : 'editor';
}
