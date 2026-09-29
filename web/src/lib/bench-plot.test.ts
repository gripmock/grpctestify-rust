import { describe, expect, it } from 'vitest';
import { benchPlot } from './bench-plot';

const tick = (elapsed_s: number, rps: number, targetRps = 0) => ({ elapsed_s, rps, targetRps });

describe('benchPlot', () => {
  it('draws one point per sample, in the box', () => {
    const plot = benchPlot([tick(0, 0), tick(5, 500), tick(10, 1000)], 300, 100);
    expect(plot?.observed).toBe('0,100 150,50 300,0');
    expect(plot?.peak).toBe(1000);
    expect(plot?.span).toBe(10);
  });

  it('draws the target beside it when the run aimed at one', () => {
    const plot = benchPlot([tick(0, 40, 50), tick(10, 48, 50)], 300, 100);
    expect(plot?.target).toBe('0,0 300,0');
    expect(plot?.peak).toBe(50);
  });

  it('says nothing about a target a closed-loop run never had', () => {
    expect(benchPlot([tick(0, 10), tick(1, 20)])?.target).toBeNull();
  });

  it('has no plot before there are two samples', () => {
    expect(benchPlot([tick(1, 100)])).toBeNull();
    expect(benchPlot([])).toBeNull();
  });

  it('refuses a series with no time between the samples', () => {
    expect(benchPlot([tick(0, 10), tick(0, 20)])).toBeNull();
  });
});
