import type { Environment } from './types';


export function substituteEnv(s: string, env: Environment | null | undefined): string {
  if (!env || !env.variables) return s;
  let result = s;
  for (const [key, val] of Object.entries(env.variables)) {
    if (!key) continue;
    result = result.replaceAll(`{{${key}}}`, val);
  }
  return result;
}


export function applyEnvironment(
  endpoint: string,
  headers: Record<string, string>,
  bodies: string[],
  env: Environment | null | undefined,
): { endpoint: string; headers: Record<string, string>; bodies: string[]; address: string | null } {
  if (!env) return { endpoint, headers, bodies, address: null };

  return {
    endpoint: substituteEnv(endpoint, env),
    headers: Object.fromEntries(
      Object.entries(headers).map(([k, v]) => [k, substituteEnv(v, env)]),
    ),
    bodies: bodies.map(b => substituteEnv(b, env)),
    address: env.address || null,
  };
}


export function findVariables(s: string): string[] {
  const matches = s.match(/\{\{(\w+)\}\}/g);
  if (!matches) return [];
  return [...new Set(matches.map(m => m.slice(2, -2)))];
}


export function mergeEnvironments(envs: Environment[]): Environment | null {
  if (envs.length === 0) return null;
  const variables: Record<string, string> = {};
  const muted = new Set<string>();
  let address: string | undefined;
  for (const env of envs) {
    for (const key of env.mutedVariables || []) muted.add(key);
    for (const [key, val] of Object.entries(env.variables)) {
      if (muted.has(key)) continue;
      variables[key] = val;
    }
    if (env.address && address === undefined) address = env.address;
  }
  return { name: envs.map(e => e.name).join('+'), variables, address };
}


export function applyEnvironmentMulti(
  endpoint: string,
  headers: Record<string, string>,
  bodies: string[],
  envs: Environment[],
): { endpoint: string; headers: Record<string, string>; bodies: string[]; address: string | null } {
  const merged = mergeEnvironments(envs);
  return applyEnvironment(endpoint, headers, bodies, merged);
}
