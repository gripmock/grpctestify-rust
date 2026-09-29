import { count } from 'luvo/data/plural';
export type PickAction = { label: string; line: string };

export function containerActions(path: string, value: unknown): PickAction[] {
  if (Array.isArray(value)) {
    return [
      { label: `Assert ${count(value.length, 'item')}`, line: `@len(${path}) == ${value.length}` },
      { label: 'Assert not empty', line: `@len(${path}) > 0` },
      { label: 'Assert present', line: `@has_value(${path})` },
    ];
  }
  return [{ label: 'Assert present', line: `@has_value(${path})` }];
}

export function isContainer(value: unknown): boolean {
  return Array.isArray(value) || (value !== null && typeof value === 'object');
}

export type AcrossStream = 'same' | 'varies' | 'missing';

function at(message: unknown, path: string): unknown {
  let value: unknown = message;
  for (const [, key, index, quoted] of path.matchAll(/\.([A-Za-z_][A-Za-z0-9_]*)|\[(\d+)\]|\["((?:[^"\\]|\\.)*)"\]/g)) {
    if (value === null || typeof value !== 'object') return undefined;
    if (index !== undefined) value = (value as unknown[])[Number(index)];
    else {
      const name = key ?? quoted!.replace(/\\(.)/g, '$1');
      value = (value as Record<string, unknown>)[name];
    }
  }
  return value;
}

export function acrossStream(messages: unknown[], path: string, value: unknown): AcrossStream {
  if (messages.length < 2) return 'same';
  const wanted = JSON.stringify(value);
  let seen = false;
  for (const message of messages) {
    const found = at(message, path);
    if (found === undefined) return 'missing';
    seen = true;
    if (JSON.stringify(found) !== wanted) return 'varies';
  }
  return seen ? 'same' : 'missing';
}

export function streamNote(state: AcrossStream, count: number): string | null {
  if (state === 'same') return null;
  return state === 'varies'
    ? `differs across the ${count} messages — an assertion is checked against each one`
    : `not in every one of the ${count} messages — an assertion is checked against each one`;
}

export function roundedNote(value: unknown): string | null {
  if (typeof value !== 'number' || !Number.isInteger(value) || Number.isSafeInteger(value)) {
    return null;
  }
  return 'wider than the panel can hold exactly — the number shown is not the one that came back';
}

export function metaActions(kind: 'headers' | 'trailers', key: string, value: string): PickAction[] {
  const fn = kind === 'headers' ? 'header' : 'trailer';
  const has = kind === 'headers' ? 'has_header' : 'has_trailer';
  const quoted = JSON.stringify(key);
  const present: PickAction = { label: 'Assert present', line: `@${has}(${quoted})` };
  if (value === '') return [present];
  return [
    { label: `Assert equals ${JSON.stringify(value)}`, line: `@${fn}(${quoted}) == ${JSON.stringify(value)}` },
    present,
  ];
}

export function statusAction(code: number): PickAction {
  return { label: `Assert the status is ${code}`, line: `@status() == ${code}` };
}

export function numberAssert(path: string, value: unknown): { line: string; label: string } | null {
  if (typeof value !== 'string') return null;
  const text = value.trim();
  if (!/^-?\d+(\.\d+)?$/.test(text)) return null;
  return { line: `${path}:number == ${text}`, label: `Assert equals ${text} as a number` };
}
