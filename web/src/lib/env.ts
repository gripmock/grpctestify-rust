import type { Environment } from './types';

const PLACEHOLDER = /\{\{([^{}]*)\}\}/g;

export function isVariableName(name: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_.]*$/.test(name);
}

export function substituteEnv(s: string, env: Environment | null | undefined): string {
  if (!env || !env.variables) return s;
  return s.replace(PLACEHOLDER, (whole, body: string) => {
    const name = body.trim();
    if (!isVariableName(name)) return whole;
    const val = env.variables[name];
    return val === undefined ? whole : val;
  });
}

export function effectiveEnvironment(env: Environment | null | undefined): Environment | null {
  if (!env) return null;
  const muted = env.mutedVariables ?? [];
  if (muted.length === 0) return env;
  return {
    ...env,
    variables: Object.fromEntries(Object.entries(env.variables).filter(([k]) => !muted.includes(k))),
  };
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

export function applyBindings(
  endpoint: string,
  headers: Record<string, string>,
  bodies: string[],
  bound: [string, string][] | undefined,
): { endpoint: string; headers: Record<string, string>; bodies: string[] } {
  if (!bound || bound.length === 0) return { endpoint, headers, bodies };
  const values = Object.fromEntries(bound);
  const one = (text: string) =>
    text.replace(PLACEHOLDER, (whole, body: string) => {
      const name = body.trim();
      if (!isVariableName(name)) return whole;
      const val = values[name];
      return val === undefined ? whole : val;
    });
  return {
    endpoint: one(endpoint),
    headers: Object.fromEntries(Object.entries(headers).map(([k, v]) => [k, one(v)])),
    bodies: bodies.map(one),
  };
}

export function unusableNames(names: string[]): string[] {
  return [...new Set(names.map(n => n.trim()).filter(n => n !== '' && !isVariableName(n)))];
}

export function resolvedNames(
  texts: string[],
  bound: [string, string][] | undefined,
  env: Environment | null | undefined,
): string[] {
  const values = new Map(bound ?? []);
  const muted = new Set(env?.mutedVariables ?? []);
  const names = new Set<string>();
  for (const text of texts) {
    for (const name of findVariables(text)) {
      const answered = values.has(name)
        || (!muted.has(name) && env?.variables?.[name] !== undefined);
      if (answered) names.add(name);
    }
  }
  return [...names];
}

export function unansweredNow(recorded: string[] | undefined, answered: string[]): string[] {
  if (!recorded || recorded.length === 0) return [];
  const has = new Set(answered);
  return recorded.filter(name => !has.has(name));
}

export function findVariables(s: string): string[] {
  const names: string[] = [];
  for (const m of s.matchAll(PLACEHOLDER)) {
    const name = m[1].trim();
    if (isVariableName(name) && !names.includes(name)) names.push(name);
  }
  return names;
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

export function mergeEnvLists(
  project: Environment[],
  browser: Environment[],
): { list: Environment[]; shadowed: string[] } {
  const taken = new Set(project.map(e => e.name));
  const shadowed = browser.filter(e => taken.has(e.name)).map(e => e.name);
  return {
    list: [...project, ...browser.filter(e => !taken.has(e.name))],
    shadowed,
  };
}
