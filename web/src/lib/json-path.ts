export function childPath(parent: string, key: string): string {
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) return `${parent}.${key}`;
  const quoted = `["${key.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"]`;
  return parent ? `${parent}${quoted}` : `.${quoted}`;
}
