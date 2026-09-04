const REGEX_FLAGS = new Set(['i', 'm', 's', 'x', 'u', 'U']);

export function withInlineFlags(pattern: string, flags: string): string {
  const kept = [...flags].filter(f => REGEX_FLAGS.has(f)).join('');
  return kept === '' ? pattern : `(?${kept})${pattern}`;
}

export function escapeFor(pattern: string): string {
  return pattern.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

export function extractLines(
  pattern: string,
  field: string,
  captures: [string, string][],
): [string, string][] {
  const escaped = escapeFor(pattern);
  return captures
    .map(([name]) => name)
    .filter(name => !/^\d+$/.test(name))
    .map(name => [name, `${field} | capture("${escaped}").${name}`]);
}
