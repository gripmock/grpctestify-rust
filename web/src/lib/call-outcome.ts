import { looksHttp } from './http-endpoint';

export interface CallOutcome {
  error?: string | null;
  statusCode?: number | null;
}

export function callFailed(response: CallOutcome | null | undefined, isHttp: boolean): boolean {
  if (!response) return false;
  if (response.error) return true;
  const code = response.statusCode;
  if (code == null) return false;
  return isHttp ? code >= 400 : code !== 0;
}

export function entryFailed(entry: {
  endpoint: string;
  collectionPath?: string | null;
  response: CallOutcome & { status?: string };
}): boolean {
  if (entry.response.status !== undefined && entry.response.status !== 'ok') return true;
  const http = looksHttp(entry.endpoint) || (entry.collectionPath ?? '').endsWith('.httf');
  return callFailed(entry.response, http);
}
