export function collectPaths(value: unknown, limit = 6, prefix = ''): string[] {
  if (value === null || typeof value !== 'object') return prefix ? [prefix] : [];

  const out: string[] = [];
  if (Array.isArray(value)) {
    if (prefix) out.push(prefix);
    if (value.length > 0) out.push(...collectPaths(value[0], limit, `${prefix}[]`));
  } else {
    for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
      const path = /^[A-Za-z_][A-Za-z0-9_]*$/.test(key) ? `${prefix}.${key}` : `${prefix}["${key}"]`;
      out.push(...collectPaths(child, limit, path));
      if (out.length >= limit) break;
    }
  }
  return out.slice(0, limit);
}

export function valueAtPath(root: unknown, path: string): unknown {
  const trimmed = path.trim();
  if (trimmed === '' || trimmed === '.') return root;
  const steps = trimmed.match(/\.[A-Za-z_][A-Za-z0-9_]*|\["[^"]*"\]|\[\d+\]/g);
  if (!steps || steps.join('') !== trimmed) return undefined;

  let cur = root;
  for (const step of steps) {
    if (cur === null || typeof cur !== 'object') return undefined;
    const key = step.startsWith('.') ? step.slice(1)
      : step.startsWith('["') ? step.slice(2, -2)
      : Number(step.slice(1, -1));
    cur = (cur as Record<string | number, unknown>)[key];
    if (cur === undefined) return undefined;
  }
  return cur;
}

export function firstStringPath(value: unknown, prefix = '', depth = 0): string | null {
  if (depth > 6 || value === null || typeof value !== 'object') return null;
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) {
      const at = `${prefix}[${i}]`;
      if (typeof value[i] === 'string') return at;
      const deeper = firstStringPath(value[i], at, depth + 1);
      if (deeper) return deeper;
    }
    return null;
  }
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    const quoted = `["${key.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"]`;
    const at = /^[A-Za-z_][A-Za-z0-9_]*$/.test(key)
      ? `${prefix}.${key}`
      : (prefix ? `${prefix}${quoted}` : `.${quoted}`);
    if (typeof child === 'string') return at;
    const deeper = firstStringPath(child, at, depth + 1);
    if (deeper) return deeper;
  }
  return null;
}
