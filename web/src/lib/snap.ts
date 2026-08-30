export type Stops = readonly number[];

export function snap(value: number, stops: Stops, radius = 12, enabled = true): number {
  if (!enabled || stops.length === 0) return value;
  let best = value;
  let bestDistance = radius;
  for (const stop of stops) {
    const distance = Math.abs(stop - value);
    if (distance <= bestDistance) {
      best = stop;
      bestDistance = distance;
    }
  }
  return best;
}

export function nextStop(value: number, stops: Stops): number {
  if (stops.length === 0) return value;
  const sorted = [...stops].sort((a, b) => a - b);
  return sorted.find(s => s > value + 1) ?? sorted[0];
}

export function collapsesAt(value: number, stops: Stops, slack = 40): boolean {
  if (stops.length === 0) return false;
  return value < Math.min(...stops) - slack;
}
