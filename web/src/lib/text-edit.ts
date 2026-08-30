import type { GctfDiagnostic } from './types';

export function applyRange(
  text: string,
  range: GctfDiagnostic['range'],
  replacement: string,
): string | null {
  const lines = text.split('\n');
  const { start, end } = range;
  if (start.line !== end.line) return null;
  const line = lines[start.line];
  if (line === undefined) return null;
  if (start.character > line.length || end.character > line.length) return null;
  if (end.character < start.character) return null;
  lines[start.line] = line.slice(0, start.character) + replacement + line.slice(end.character);
  return lines.join('\n');
}

export function rewriteOf(problem: GctfDiagnostic): string | null {
  for (const key of ['replacement', 'suggested_key'] as const) {
    const said = problem.data?.[key];
    if (typeof said === 'string' && said !== '') return said;
  }
  return null;
}

export function applyRewrites(
  text: string,
  problems: GctfDiagnostic[],
): { text: string; applied: number } {
  const edits = problems
    .map(problem => ({ problem, rewrite: rewriteOf(problem) }))
    .filter((e): e is { problem: GctfDiagnostic; rewrite: string } => e.rewrite !== null)
    .sort((a, b) => (b.problem.range.start.line - a.problem.range.start.line)
      || (b.problem.range.start.character - a.problem.range.start.character));

  let out = text;
  let applied = 0;
  for (const { problem, rewrite } of edits) {
    const next = applyRange(out, problem.range, rewrite);
    if (next === null) continue;
    out = next;
    applied += 1;
  }
  return { text: out, applied };
}
