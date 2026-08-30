import { describe, it, expect } from 'vitest';
import { stoppedShort, BENCH_GROUPS, fieldsInUse, fieldsToAdd, latencyNote, stopCondition, validDuration, validNumber, validThreshold, thresholdsOf, isThresholdKey, groupApplies } from './bench-model';

describe('BENCH_GROUPS', () => {
  it('only names keys the validator supports', () => {
    const keys = BENCH_GROUPS.flatMap(g => g.fields.map(f => f.key));
    expect(new Set(keys).size).toBe(keys.length);
    expect(keys).toContain('concurrency');
    expect(keys).toContain('load_schedule');
    expect(keys).not.toContain('concurrency-schedule');
  });

  it('marks the concurrency sweep as the only source of levels[]', () => {
    const field = BENCH_GROUPS.flatMap(g => g.fields).find(f => f.key === 'concurrency_schedule');
    expect(field?.hint).toContain('levels[]');
  });
});

describe('validDuration', () => {
  it('accepts the suffixes the validator accepts', () => {
    for (const v of ['5', '5s', '250ms', '1.5m', '2h', '']) expect(validDuration(v)).toBe(true);
  });
  it('rejects the rest', () => {
    for (const v of ['5x', 'abc', '-1s']) expect(validDuration(v)).toBe(false);
  });
});

describe('validNumber', () => {
  it('accepts digit separators, as the validator does', () => {
    expect(validNumber('10_000')).toBe(true);
    expect(validNumber('50')).toBe(true);
  });
  it('rejects negatives and words', () => {
    expect(validNumber('-1')).toBe(false);
    expect(validNumber('many')).toBe(false);
  });
});

describe('thresholds', () => {
  it('accepts only the comparison grammar', () => {
    for (const v of ['<100', '<= 250', '>0.5', '>=99']) expect(validThreshold(v)).toBe(true);
    for (const v of ['100', '~100', '<']) expect(validThreshold(v)).toBe(false);
  });

  it('finds threshold keys among the rest', () => {
    expect(thresholdsOf({ concurrency: '10', 'thresholds.latency_ms.p99': '<250', thresholds: '<1' }))
      .toEqual([['thresholds.latency_ms.p99', '<250'], ['thresholds', '<1']]);
    expect(isThresholdKey('thresholds.pass_rate_pct')).toBe(true);
    expect(isThresholdKey('concurrency')).toBe(false);
  });
});

describe('groupApplies', () => {
  const schedule = BENCH_GROUPS.find(g => g.title === 'schedule')!;
  const shape = BENCH_GROUPS.find(g => g.title === 'shape')!;

  it('shows every other group whatever the mode', () => {
    expect(groupApplies(shape, { mode: 'fixed' })).toBe(true);
  });

  it('hides the schedule of a fixed bench — nine empty inputs it cannot use', () => {
    expect(groupApplies(schedule, { mode: 'fixed', concurrency: '4' })).toBe(false);
  });

  it('shows it for a mode that ramps', () => {
    expect(groupApplies(schedule, { mode: 'stepping' })).toBe(true);
    expect(groupApplies(schedule, { mode: 'adaptive' })).toBe(true);
  });

  it('shows it whenever anything in it is already set', () => {
    expect(groupApplies(schedule, { mode: 'fixed', load_start: '10' })).toBe(true);
    expect(groupApplies(schedule, { mode: 'fixed', concurrency_schedule: 'step' })).toBe(true);
  });

  it('reads no mode as fixed', () => {
    expect(groupApplies(schedule, {})).toBe(false);
    expect(groupApplies(schedule, { load_step: '5' })).toBe(true);
  });
});

describe('stopCondition', () => {
  it('is the duration when one is set, and says the requests are ignored', () => {
    expect(stopCondition({ duration: '30s', requests: '1000' }))
      .toEqual({ governs: 'duration', ignored: 'requests' });
  });

  it('ignores nothing when only one of the two is set', () => {
    expect(stopCondition({ duration: '30s' })).toEqual({ governs: 'duration', ignored: null });
    expect(stopCondition({ requests: '1000' })).toEqual({ governs: 'requests', ignored: null });
  });

  it('is nothing when neither is set', () => {
    expect(stopCondition({})).toEqual({ governs: 'none', ignored: null });
    expect(stopCondition({ duration: '  ' , requests: '' })).toEqual({ governs: 'none', ignored: null });
  });
});

describe('latencyNote', () => {
  it('says what the numbers are, when nothing succeeded', () => {
    expect(latencyNote({ count: 1000, ok: 0 })).toBe('over failed requests only — nothing succeeded');
  });

  it('says nothing when something did', () => {
    expect(latencyNote({ count: 1000, ok: 1 })).toBeNull();
    expect(latencyNote({ count: 1000, ok: 1000 })).toBeNull();
  });

  it('says nothing about a run with no requests', () => {
    expect(latencyNote({ count: 0, ok: 0 })).toBeNull();
  });
});

describe('what the bench editor shows', () => {
  const shape = BENCH_GROUPS.find(g => g.title === 'shape')!;

  it('shows the fields this file sets, and no others', () => {
    const shown = fieldsInUse(shape, { mode: 'fixed', concurrency: '4' }, []).map(f => f.key);
    expect(shown).toEqual(['mode', 'concurrency']);
  });

  it('shows a field opened by hand, empty as it is', () => {
    const shown = fieldsInUse(shape, { concurrency: '4' }, ['max_rps']).map(f => f.key);
    expect(shown).toEqual(['concurrency', 'max_rps']);
  });

  it('offers what is left, grouped the way the editor groups it', () => {
    const groups = fieldsToAdd({ mode: 'fixed', concurrency: '4' }, ['max_rps']);
    const offered = groups.flatMap(g => g.fields.map(f => f.key));
    expect(offered).not.toContain('mode');
    expect(offered).not.toContain('concurrency');
    expect(offered).not.toContain('max_rps');
    expect(offered).toContain('warmup');
    expect(groups.every(g => g.fields.length > 0)).toBe(true);
  });
});

describe('a bench that was stopped', () => {
  it('says how far it got, against the plan it was given', () => {
    expect(stoppedShort('cancelled', 2040, '6s'))
      .toBe('Stopped after 2.0 s of a 6s plan — these numbers are what it got through, not the run that was asked for.');
  });

  it('says what it got through when there is no plan to compare', () => {
    expect(stoppedShort('cancelled', 2040, undefined))
      .toBe('Stopped before it finished — these numbers are the 2.0 s it got through, not the run that was asked for.');
    expect(stoppedShort('cancelled', 2040, 'forever'))
      .toContain('these numbers are the 2.0 s it got through');
  });

  it('says nothing about a run that finished', () => {
    expect(stoppedShort('passed', 6000, '6s')).toBeNull();
    expect(stoppedShort(null, 6000, '6s')).toBeNull();
    expect(stoppedShort('failed', 6000, '6s')).toBeNull();
  });
});
