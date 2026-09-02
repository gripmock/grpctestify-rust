import type { Environment } from './types';
import { blankRow, type Origin, type Row } from './env-rows';
import { looksLikeSecret } from './secret-names';

export type EnvView =
  | { kind: 'list' }
  | { kind: 'edit'; name: string; origin: Origin }
  | { kind: 'new' };

export interface EnvSeed {
  view: EnvView;
  rows: Row[];
  name: string;
  address: string;
  tls: boolean | undefined;
  tlsOpen: boolean;
  tlsCa: string;
  tlsCert: string;
  tlsKey: string;
  tlsInsecure: boolean;
}

export function browserSeed(env: Environment, extra?: string | null, extraValue = ''): EnvSeed {
  const loaded: Row[] = Object.entries(env.variables).map(([key, value]) => ({ key, value, local: false }));
  return {
    view: { kind: 'edit', name: env.name, origin: 'browser' },
    rows: [
      ...loaded,
      ...(extra && !loaded.some(r => r.key === extra) ? [{ key: extra, value: extraValue, local: false }] : []),
      blankRow(),
    ],
    name: env.name,
    address: env.address || '',
    tls: env.tls,
    tlsOpen: env.tls !== undefined,
    tlsCa: env.tlsCa || '',
    tlsCert: env.tlsCert || '',
    tlsKey: env.tlsKey || '',
    tlsInsecure: env.tlsInsecure ?? false,
  };
}

export function newSeed(defineVar: string, defineValue?: string): EnvSeed {
  return {
    view: { kind: 'new' },
    rows: [{ key: defineVar, value: defineValue ?? '', local: looksLikeSecret(defineVar) }, blankRow()],
    name: '',
    address: '',
    tls: undefined,
    tlsOpen: false,
    tlsCa: '',
    tlsCert: '',
    tlsKey: '',
    tlsInsecure: false,
  };
}

export function openingSeed(
  defineVar: string | null | undefined,
  defineValue: string | undefined,
  active: Environment | undefined,
): EnvSeed | null {
  if (!defineVar) return null;
  if (!active) return newSeed(defineVar, defineValue);
  if (active.source === 'browser') return browserSeed(active, defineVar, defineValue);
  return null;
}
