import type { CallResult, HistoryEntry, WireProtocol } from './types';

export interface ProjectEntry extends HistoryEntry {
  session: string;
}

interface RawEntry {
  id?: string;
  timestamp?: number;
  endpoint?: string;
  kind?: string;
  collection_path?: string;
  dataset_row?: number | null;
  bodies?: string[];
  headers?: Record<string, string>;
  connection?: { address?: string; protocol?: string; tls?: boolean };
  response?: Partial<CallResult> & {
    status?: string;
    status_code?: number | null;
    duration_ms?: number | null;
    shape?: string | null;
    assertions_passed?: number;
    assertions_total?: number;
  };
}

const emptyResult = (): CallResult => ({
  status: 'ok', statusCode: null, messages: [], headers: {}, trailers: {}, error: null, durationMs: null,
});

export function flattenProjectHistory(payload: unknown): ProjectEntry[] {
  if (!payload || typeof payload !== 'object') return [];
  const out: ProjectEntry[] = [];

  for (const [session, lines] of Object.entries(payload as Record<string, unknown>)) {
    if (!Array.isArray(lines)) continue;
    for (const raw of lines as RawEntry[]) {
      const response = raw.response ?? {};
      out.push({
        id: raw.id ?? `${session}:${out.length}`,
        timestamp: raw.timestamp ?? 0,
        endpoint: raw.endpoint ?? raw.collection_path ?? '',
        bodies: raw.bodies ?? [],
        headers: raw.headers ?? {},
        response: {
          ...emptyResult(),
          ...response,
          status: response.status === 'error' ? 'error' : 'ok',
          statusCode: response.statusCode ?? response.status_code ?? null,
          durationMs: response.durationMs ?? response.duration_ms ?? null,
          shape: response.shape ?? null,
        },
        session,
        ...(raw.kind === 'run' ? { kind: 'run' as const } : {}),
        ...(raw.connection?.address
          ? {
              connection: {
                address: raw.connection.address,
                tls: raw.connection.tls ?? false,
                ...(raw.connection.protocol
                  ? { protocol: raw.connection.protocol as WireProtocol }
                  : {}),
              },
            }
          : {}),
        ...(typeof response.assertions_total === 'number' && response.assertions_total > 0
          ? { checks: { passed: response.assertions_passed ?? 0, total: response.assertions_total } }
          : {}),
        ...(raw.collection_path ? { collectionPath: raw.collection_path } : {}),
        ...(typeof raw.dataset_row === 'number' ? { datasetRow: raw.dataset_row } : {}),
      });
    }
  }

  return out.sort((a, b) => b.timestamp - a.timestamp);
}
