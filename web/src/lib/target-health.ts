export interface TargetHealth {
  reachable: boolean;
  ms: number;
  detail: string | null;
  dialled: string;
}

export function healthNote(health: TargetHealth | null, probing: boolean): string {
  if (probing) return 'trying the socket…';
  if (!health) return '';
  if (health.dialled === '') return health.detail ?? 'nothing to try';
  return health.reachable
    ? `something is listening on ${health.dialled} — ${health.ms} ms to open a socket`
    : `nothing answered on ${health.dialled}${health.detail ? ` — ${health.detail}` : ''}`;
}

export async function probeTarget(address: string, signal?: AbortSignal): Promise<TargetHealth | null> {
  try {
    const res = await fetch('/api/target-health', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ address }),
      signal,
    });
    if (!res.ok) return null;
    return (await res.json()) as TargetHealth;
  } catch {
    return null;
  }
}
