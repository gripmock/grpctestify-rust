import type { CollectionParsed } from './types';

export interface MessageRun {
  skipped: boolean;
  repeat: number | null;
}

export function messageRun(
  parsed: CollectionParsed | null,
  index: number,
  family: 'gctf' | 'httf' = 'gctf',
): MessageRun {
  const run = sectionRun(parsed, 'REQUEST', parsed?.bodies_stream ? 0 : index);
  return family === 'httf' ? { ...run, repeat: null } : run;
}

export function sectionRun(
  parsed: CollectionParsed | null,
  section: string,
  index = 0,
): MessageRun {
  const on = (parsed?.attributes ?? []).filter(a => a.section === section && a.index === index);
  const repeat = Number(on.find(a => a.name === 'repeat')?.value);
  return {
    skipped: on.some(a => a.name === 'skip' && a.value !== 'false'),
    repeat: Number.isFinite(repeat) && repeat > 1 ? Math.floor(repeat) : null,
  };
}

export function everyMessageSkipped(
  parsed: CollectionParsed | null,
  bodies: number,
  family: 'gctf' | 'httf' = 'gctf',
): boolean {
  if (family === 'httf' || bodies === 0) return false;
  if (parsed?.bodies_stream) return sectionRun(parsed, 'REQUEST', 0).skipped;
  for (let i = 0; i < bodies; i++) if (!sectionRun(parsed, 'REQUEST', i).skipped) return false;
  return true;
}
