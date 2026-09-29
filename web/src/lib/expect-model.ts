import type { CollectionParsed } from './types';

export type ExpectMode = 'none' | 'response' | 'error';

export function expectMode(p: CollectionParsed | null): ExpectMode {
  if (p?.expect_error) return 'error';
  if ((p?.expect_responses?.length ?? 0) > 0) return 'response';
  return 'none';
}

export function errorExpectBody(code: number | null, error: string | null): string {
  const body: { code?: number; message?: string } = {};
  if (code !== null && code !== 0) body.code = code;
  const message = (error ?? '').trim();
  if (message !== '') body.message = message;
  return JSON.stringify(body, null, 2);
}

export type Disagreement = 'expects-failure-got-ok' | 'expects-messages-got-failure' | null;

export function expectDisagreement(
  mode: ExpectMode,
  response: { failed: boolean } | null,
): Disagreement {
  if (!response) return null;
  if (mode === 'error' && !response.failed) return 'expects-failure-got-ok';
  if (mode === 'response' && response.failed) return 'expects-messages-got-failure';
  return null;
}

export function disagreementNote(kind: Disagreement): string | null {
  switch (kind) {
    case 'expects-failure-got-ok':
      return 'This file expects the call to fail — the last one came back ok, so a run would fail here.';
    case 'expects-messages-got-failure':
      return 'This file expects messages — the last call failed, so a run would fail here.';
    default:
      return null;
  }
}

export function expectBody(message: unknown, raw?: string): string {
  if (typeof message === 'string') return message;
  if (raw !== undefined && raw.trim() !== '') {
    const exact = losslessPretty(raw);
    if (exact !== null) return exact;
  }
  return JSON.stringify(message, null, 2);
}

export function numbersRounded(raw: string | undefined): boolean {
  if (!raw) return false;
  for (const literal of raw.match(/-?\d{16,}(?:\.\d+)?(?:[eE][+-]?\d+)?/g) ?? []) {
    if (String(Number(literal)) !== literal) return true;
  }
  return false;
}

function losslessPretty(raw: string): string | null {
  try {
    const parsed = JSON.parse(raw);
    const printed = JSON.stringify(parsed);
    const normalised = JSON.stringify(JSON.parse(raw.trim()));
    if (printed !== normalised) return raw.trim();
    for (const literal of raw.match(/-?\d{16,}(?:\.\d+)?(?:[eE][+-]?\d+)?/g) ?? []) {
      if (String(Number(literal)) !== literal) return raw.trim();
    }
    return JSON.stringify(parsed, null, 2);
  } catch {
    return null;
  }
}
