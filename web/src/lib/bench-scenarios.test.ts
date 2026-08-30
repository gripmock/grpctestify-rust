import { describe, expect, it } from 'vitest';
import { activeScenario, applyScenario, benchKeys, scenarioKeys, scenariosOf } from './bench-scenarios';

const served = [
  { name: 'functional', description: 'Quick functional check', keys: [['mode', 'fixed'], ['concurrency', '1'], ['requests', '100']] as [string, string][] },
  { name: 'load', description: 'Stepped load test 50→200 RPS', keys: [['mode', 'stepping'], ['load_schedule', 'step'], ['load_start', '50'], ['load_end', '200']] as [string, string][] },
  { name: 'sweep', description: 'Concurrency sweep 1→64', keys: [['mode', 'fixed'], ['requests', '200'], ['concurrency_schedule', 'step'], ['concurrency_start', '1'], ['concurrency_end', '64']] as [string, string][] },
];
const scenarios = scenariosOf(served);

describe('the profiles the runner serves', () => {
  it('reads them as the cards need them', () => {
    expect(scenarios[0]).toEqual({
      name: 'functional',
      description: 'Quick functional check',
      keys: { mode: 'fixed', concurrency: '1', requests: '100' },
    });
  });

  it('writes only keys the form knows', () => {
    const known = new Set(benchKeys());
    expect(scenarioKeys(scenarios).filter(k => !known.has(k))).toEqual([]);
  });
});

describe('applyScenario', () => {
  it('replaces the shape and keeps what the author set', () => {
    const before = { concurrency: '4', 'thresholds.latency_ms.p99': '< 250', warmup: '5s' };
    const after = applyScenario(before, scenarios[0], scenarios);
    expect(after['thresholds.latency_ms.p99']).toBe('< 250');
    expect(after.warmup).toBe('5s');
    expect(after.concurrency).toBe('1');
    expect(after.requests).toBe('100');
  });

  it('leaves no key from the shape it replaced', () => {
    const load = applyScenario({}, scenarios[1], scenarios);
    const functional = applyScenario(load, scenarios[0], scenarios);
    expect(functional.load_schedule).toBeUndefined();
    expect(functional.load_start).toBeUndefined();
  });
});

describe('activeScenario', () => {
  it('recognises a config that is exactly one of them', () => {
    expect(activeScenario(applyScenario({}, scenarios[2], scenarios), scenarios)).toBe('sweep');
  });

  it('claims nothing for a config of the author’s own', () => {
    expect(activeScenario({ mode: 'fixed', concurrency: '7' }, scenarios)).toBeNull();
  });

  it('claims nothing while the list has not arrived', () => {
    expect(activeScenario({ mode: 'fixed' }, [])).toBeNull();
  });
});
