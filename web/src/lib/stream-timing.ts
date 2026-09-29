export interface Arrival {
  at: number;
  gap: number | null;
}

export const NOTABLE_GAP_MS = 100;

export function arrivals(offsets: number[]): Arrival[] {
  return offsets.map((at, i) => ({
    at,
    gap: i === 0 ? null : Math.max(0, at - offsets[i - 1]),
  }));
}

export function isNotable(gap: number | null): boolean {
  return gap !== null && gap >= NOTABLE_GAP_MS;
}
