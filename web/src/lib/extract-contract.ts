import type { DocumentSummary } from './types';

export type ExtractReach =
  | { kind: 'steps'; steps: number[] }
  | { kind: 'asserts' }
  | { kind: 'none' };

function mentions(name: string, line: string): boolean {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`(^|[^A-Za-z0-9_.])${escaped}([^A-Za-z0-9_.]|$)`).test(line)
    || line.includes(`{{${name}}}`);
}

export function reachOf(
  name: string,
  documents: DocumentSummary[],
  step: number,
  asserts: string[] = [],
): ExtractReach {
  const steps = documents
    .map((doc, i) => ({ i, uses: (doc.consumes ?? []).includes(name) }))
    .filter(d => d.i > step && d.uses)
    .map(d => d.i);
  if (steps.length > 0) return { kind: 'steps', steps };
  if (asserts.some(line => mentions(name, line))) return { kind: 'asserts' };
  return { kind: 'none' };
}

export function reachLabel(reach: ExtractReach): string {
  switch (reach.kind) {
    case 'steps': return `→ ${reach.steps.map(s => `step ${s + 1}`).join(', ')}`;
    case 'asserts': return 'asserts';
    case 'none': return 'unread';
  }
}

export function reachTitle(name: string, reach: ExtractReach): string {
  switch (reach.kind) {
    case 'steps': return `${reach.steps.map(s => `Step ${s + 1}`).join(' and ')} reads {{${name}}}`;
    case 'asserts': return `This document's ASSERTS read ${name} — grpctestify check still reports it as an unused variable, which counts later documents only`;
    case 'none': return `Nothing reads ${name} — grpctestify check reports it as an unused variable`;
  }
}

export function extractAudience(documentCount: number, step = 0): string {
  return documentCount > step + 1
    ? 'later steps read it as {{name}}'
    : 'nothing reads it yet, because this file ends here — a step added after it would';
}

export function extractAudienceEmpty(documentCount: number, step = 0): string {
  return documentCount > step + 1
    ? 'later steps read them as {{name}}'
    : 'nothing would read one yet, because this file ends here';
}

export interface PreviewSource {
  ok: boolean;
  note: string;
}

export function previewSource(
  response: { status: string; messages: unknown[]; fromStep?: number } | null | undefined,
  step: number,
  messages: number,
  ranAndBound = false,
): PreviewSource {
  if (!response || response.status === 'pending' || messages === 0) {
    return ranAndBound
      ? { ok: false, note: 'What this file has bound' }
      : { ok: false, note: 'Execute this step to see what these would take' };
  }
  if (response.fromStep === undefined) {
    return { ok: false, note: 'The response on screen came from a run of the whole file — execute this step to check these' };
  }
  if (response.fromStep !== step) {
    return { ok: false, note: `The response on screen is from step ${response.fromStep + 1} — execute this step to check these` };
  }
  return {
    ok: true,
    note: messages > 1
      ? `What these take from message ${messages} of ${messages} — the one the runner extracts from`
      : 'What these take from the last response',
  };
}

export function extractionInput(messages: unknown[]): { message: unknown; index: number; total: number } | null {
  if (!messages || messages.length === 0) return null;
  return { message: messages[messages.length - 1], index: messages.length - 1, total: messages.length };
}

export function ranValue(ran: [string, string][] | undefined, name: string): string | null {
  return ran?.find(([bound]) => bound === name)?.[1] ?? null;
}

export function flowLabel(name: string, value: string | null, room = 14): string {
  if (value === null) return name;
  const oneLine = value.replace(/\s+/g, ' ').trim();
  return oneLine !== '' && oneLine.length <= room ? `${name} = ${oneLine}` : name;
}

export function flowTitle(name: string, value: string | null): string {
  return value === null
    ? `${name} — bound here and read by a later step`
    : `${name} = ${value} — what this file carried forward`;
}
