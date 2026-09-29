import type { ReflectionMethod } from './types';

export function groupMethods(methods: ReflectionMethod[]): [string, ReflectionMethod[]][] {
  const map = new Map<string, ReflectionMethod[]>();
  for (const m of methods) {
    const service = m.fullName.split('/')[0] || m.service;
    const group = map.get(service);
    if (group) group.push(m);
    else map.set(service, [m]);
  }
  return [...map.entries()];
}

export function matchesQuery(fullName: string, query: string): boolean {
  const haystack = fullName.toLowerCase();
  return query.toLowerCase().split(/\s+/).filter(Boolean).every(token => haystack.includes(token));
}
