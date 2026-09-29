import type { GctfDiagnostic } from './types';

export function problemsFor(assertion: string, diagnostics: GctfDiagnostic[]): GctfDiagnostic[] {
  const line = assertion.trim();
  if (line === '') return [];
  return diagnostics.filter(d => d.message.trimEnd().endsWith(`: ${line}`));
}

export function unboundLines(diagnostics: GctfDiagnostic[]): string[] {
  const mark = 'EXTRACT line binds nothing — write `name = filter`: ';
  return diagnostics
    .filter(d => d.message.includes(mark))
    .map(d => d.message.slice(d.message.indexOf(mark) + mark.length).trim())
    .filter(line => line !== '');
}

export function droppedLines(diagnostics: GctfDiagnostic[], section: string): string[] {
  const mark = `${section} line is not a \`key: value\` pair, so it is dropped: `;
  return diagnostics
    .filter(d => d.message.includes(mark))
    .map(d => d.message.slice(d.message.indexOf(mark) + mark.length).trim())
    .filter(line => line !== '');
}

export function missingPaths(
  diagnostics: GctfDiagnostic[],
  section: 'PROTO' | 'TLS',
): { named: string; at: string | null }[] {
  const pattern = new RegExp(`^${section} names (.+?), and there is nothing (?:at (.+)|there)$`);
  return diagnostics
    .map(d => pattern.exec(d.message.trim()))
    .filter((m): m is RegExpExecArray => m !== null)
    .map(m => ({ named: m[1]!, at: m[2] ?? null }));
}

export function keyProblem(key: string, diagnostics: GctfDiagnostic[]): string | null {
  const about = `key '${key}'`;
  return diagnostics.find(d => d.message.includes(about))?.message ?? null;
}

export function methodProblem(method: string, diagnostics: GctfDiagnostic[]): string | null {
  const named = `'${method.trim().toUpperCase()}' is not one of the usual HTTP methods`;
  return diagnostics.find(d => d.message.includes(named))?.message ?? null;
}
