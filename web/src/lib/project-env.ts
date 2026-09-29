export interface ProjectEnvFile {
  content: string;
  secret: string[];
}

function names(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((n): n is string => typeof n === 'string') : [];
}

export function projectEnvFile(payload: unknown): ProjectEnvFile {
  if (typeof payload === 'string') return { content: payload, secret: [] };
  if (!payload || typeof payload !== 'object') return { content: '', secret: [] };
  const said = payload as { content?: unknown; secret?: unknown };
  return {
    content: typeof said.content === 'string' ? said.content : '',
    secret: names(said.secret),
  };
}

export function projectEnvLocal(payload: unknown): { exists: boolean; content: string | null; secret: string[] } {
  if (!payload || typeof payload !== 'object') return { exists: false, content: null, secret: [] };
  const said = payload as { exists?: unknown; content?: unknown; secret?: unknown };
  return {
    exists: said.exists === true,
    content: typeof said.content === 'string' ? said.content : null,
    secret: names(said.secret),
  };
}
