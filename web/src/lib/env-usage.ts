import { findVariables } from './env';
import type { Environment } from './types';

export type VarSource = 'env' | 'dataset' | 'extract' | 'project' | 'source' | 'run' | 'unknown';

export interface VarUse {
  key: string;
  value?: string;
  muted: boolean;
  resolved: boolean;
  from: VarSource;
  empty?: boolean;
  runOnly?: boolean;
}

export interface RuntimeNames {
  datasetColumns?: string[];
  extracted?: string[];
  projectNames?: string[];
  datasetRowValues?: Record<string, string> | null;
  sourceColumns?: string[];
  runBound?: [string, string][];
  mode?: 'execute' | 'run';
}

export function resolvedElsewhere(
  key: string,
  runtime: RuntimeNames,
): 'dataset' | 'extract' | 'project' | 'source' | null {
  const column = key.startsWith('dataset.') ? key.slice('dataset.'.length) : null;
  if (column !== null && (runtime.datasetColumns ?? []).includes(column)) return 'dataset';
  if ((runtime.sourceColumns ?? []).includes(key)) return 'source';
  if ((runtime.extracted ?? []).includes(key)) return 'extract';
  if ((runtime.projectNames ?? []).includes(key)) return 'project';
  return null;
}

export function envUsage(
  text: string,
  env: Environment | null | undefined,
  runtime: RuntimeNames = {},
): VarUse[] {
  if (!text) return [];
  const muted = new Set(env?.mutedVariables || []);

  return findVariables(text).map(key => {
    const value = env?.variables[key];
    const isMuted = muted.has(key);

    const bound = (runtime.runBound ?? []).find(([name]) => name === key);
    if (bound) {
      return { key, value: bound[1], muted: isMuted, resolved: true, from: 'run', ...(bound[1] === '' ? { empty: true } : {}) };
    }

    if (value !== undefined && !isMuted) {
      return { key, value, muted: isMuted, resolved: true, from: 'env', ...(value === '' ? { empty: true } : {}) };
    }

    const column = key.startsWith('dataset.') ? key.slice('dataset.'.length) : null;
    if (column !== null && runtime.datasetRowValues && column in runtime.datasetRowValues) {
      const held = runtime.datasetRowValues[column];
      return { key, value: held, muted: isMuted, resolved: true, from: 'dataset', ...(held === '' ? { empty: true } : {}) };
    }

    const from = resolvedElsewhere(key, runtime);
    if (from === 'project') return { key, value, muted: isMuted, resolved: true, from };
    if (from) {
      const runOnly = runtime.mode === 'execute';
      return { key, value, muted: isMuted, resolved: !runOnly, from, ...(runOnly ? { runOnly } : {}) };
    }
    return { key, value, muted: isMuted, resolved: false, from: 'unknown' };
  });
}

export function unresolvedCount(uses: VarUse[]): number {
  return uses.filter(u => !u.resolved).length;
}
