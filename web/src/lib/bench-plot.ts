export interface Tick {
  elapsed_s: number;
  rps: number;
  targetRps: number;
}

export interface Plot {
  observed: string;
  target: string | null;
  peak: number;
  span: number;
}

export function benchPlot(ticks: Tick[], width = 300, height = 100): Plot | null {
  if (ticks.length < 2) return null;
  const span = Math.max(...ticks.map(t => t.elapsed_s));
  const targeted = ticks.some(t => t.targetRps > 0);
  const peak = Math.max(...ticks.map(t => Math.max(t.rps, targeted ? t.targetRps : 0)), 1);
  if (span <= 0) return null;

  const at = (t: Tick, value: number) => {
    const x = (t.elapsed_s / span) * width;
    const y = height - (value / peak) * height;
    return `${round(x)},${round(y)}`;
  };
  return {
    observed: ticks.map(t => at(t, t.rps)).join(' '),
    target: targeted ? ticks.map(t => at(t, t.targetRps)).join(' ') : null,
    peak,
    span,
  };
}

function round(n: number): number {
  return Math.round(n * 10) / 10;
}
