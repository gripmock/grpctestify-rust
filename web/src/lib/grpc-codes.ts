export const GRPC_CODES: { code: number; name: string }[] = [
  { code: 1, name: 'CANCELLED' },
  { code: 2, name: 'UNKNOWN' },
  { code: 3, name: 'INVALID_ARGUMENT' },
  { code: 4, name: 'DEADLINE_EXCEEDED' },
  { code: 5, name: 'NOT_FOUND' },
  { code: 6, name: 'ALREADY_EXISTS' },
  { code: 7, name: 'PERMISSION_DENIED' },
  { code: 8, name: 'RESOURCE_EXHAUSTED' },
  { code: 9, name: 'FAILED_PRECONDITION' },
  { code: 10, name: 'ABORTED' },
  { code: 11, name: 'OUT_OF_RANGE' },
  { code: 12, name: 'UNIMPLEMENTED' },
  { code: 13, name: 'INTERNAL' },
  { code: 14, name: 'UNAVAILABLE' },
  { code: 15, name: 'DATA_LOSS' },
  { code: 16, name: 'UNAUTHENTICATED' },
];

export function codeName(code: number | null): string | null {
  return GRPC_CODES.find(c => c.code === code)?.name ?? null;
}

export type ErrorShape = { code: number | null; message: string | null; extra: boolean };

export function readErrorBody(body: string): ErrorShape | null {
  let value: unknown;
  try { value = JSON.parse(body); } catch { return null; }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  const o = value as Record<string, unknown>;
  const code = typeof o.code === 'number' ? o.code : null;
  const message = typeof o.message === 'string' ? o.message : null;
  const known = Object.keys(o).every(
    k => (k === 'code' && typeof o.code === 'number') || (k === 'message' && typeof o.message === 'string'),
  );
  return { code, message, extra: !known };
}

export function writeErrorField(body: string, field: 'code' | 'message', value: number | string | null): string {
  let o: Record<string, unknown> = {};
  try {
    const parsed = JSON.parse(body);
    if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) o = parsed as Record<string, unknown>;
  } catch { /* an unparseable body is replaced by the field being set */ }
  if (value === null || value === '') delete o[field];
  else o[field] = value;
  return JSON.stringify(o, null, 2);
}
