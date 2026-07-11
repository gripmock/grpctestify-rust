
export function parseDotenv(text: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const eqIdx = trimmed.indexOf('=');
    if (eqIdx === -1) continue;
    const key = trimmed.slice(0, eqIdx).trim();
    const val = trimmed.slice(eqIdx + 1).trim();
    if (key) result[key] = val;
  }
  return result;
}


export function formatDotenv(vars: Record<string, string>): string {
  const keys = Object.keys(vars).sort((a, b) => {
    if (a === 'GRPC_ADDRESS') return -1;
    if (b === 'GRPC_ADDRESS') return 1;
    return a.localeCompare(b);
  });
  return keys.map(k => `${k}=${vars[k]}`).join('\n') + '\n';
}


export function mergeEnv(
  shared: Record<string, string>,
  local: Record<string, string>,
): Record<string, string> {
  return { ...shared, ...local };
}


export function localOverrideKeys(
  shared: Record<string, string>,
  local: Record<string, string>,
): string[] {
  return Object.keys(local).filter(k => local[k] !== shared[k]);
}
