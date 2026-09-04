import { count } from 'luvo/data/plural';
export type DropOutcome =
  | { kind: 'schema'; name: string }
  | { kind: 'opened'; name: string }
  | { kind: 'refused'; name: string; reason: string };

export function summariseDrop(
  outcomes: DropOutcome[],
  options?: { fileOpen?: boolean },
): { text: string; failed: boolean } | null {
  if (outcomes.length === 0) return null;
  const wiring = options?.fileOpen ? ' — pick it in the file’s PROTO section' : '';
  if (outcomes.length === 1) {
    const only = outcomes[0];
    if (only.kind === 'refused') return { text: `${only.name}: ${only.reason}`, failed: true };
    const what = only.kind === 'schema' ? 'added to the collections' : 'opened as a tab';
    const next = only.kind === 'schema' ? wiring : '';
    return { text: `${only.name} ${what}${next}`, failed: false };
  }

  const schemas = outcomes.filter(o => o.kind === 'schema').length;
  const opened = outcomes.filter(o => o.kind === 'opened').length;
  const refused = outcomes.filter(o => o.kind === 'refused');
  const parts: string[] = [];
  if (schemas > 0) {
    parts.push(`${count(schemas, 'schema')} added${
      options?.fileOpen ? ' — pick them in the file’s PROTO section' : ''}`);
  }
  if (opened > 0) parts.push(`${count(opened, 'file')} opened`);
  if (refused.length > 0) {
    parts.push(`${refused.length} refused — ${refused.map(r => r.name).join(', ')}`);
  }
  return { text: parts.join(' · '), failed: refused.length > 0 };
}
