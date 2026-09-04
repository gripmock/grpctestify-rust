import { describe, expect, it } from 'vitest';
import {
  KINDS, MAX_BANK, TIER, clockLabel, emptyRound, makeHole, missed, reads, running, scoreLine, scored,
  secondsFor, solved, startClock, verdictOf,
} from './golf';

function seeded(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

describe('the holes a round is made of', () => {
  it('are generated, so a second round is not the same round', () => {
    const a = makeHole(seeded(1));
    const b = makeHole(seeded(2));
    expect(JSON.stringify(a) === JSON.stringify(b)).toBe(false);
  });

  it('states a par that a real expression could hit, whatever it generated', () => {
    for (const kind of KINDS) {
      for (let seed = 1; seed <= 25; seed++) {
        const hole = makeHole(seeded(seed), kind);
        expect(hole.par, kind).toBeGreaterThan(0);
        expect(hole.ask.length, kind).toBeGreaterThan(0);
        expect(hole.want, kind).not.toBeUndefined();
        expect(JSON.stringify(hole.input), kind).not.toBe('{}');
      }
    }
  });

  it('asks for something the input holds, whenever the answer is a value to pick out', () => {
    for (const kind of ['field', 'nested', 'last']) {
      for (let seed = 1; seed <= 15; seed++) {
        const hole = makeHole(seeded(seed), kind);
        expect(JSON.stringify(hole.input).includes(String(hole.want)), `${kind}: ${hole.want}`).toBe(true);
      }
    }
  });
});

describe('a hole', () => {
  const hole = makeHole(seeded(3), 'field');

  it('is solved by one output that equals what was asked for', () => {
    expect(solved(hole, [hole.want])).toBe(true);
    expect(solved(hole, ['nope'])).toBe(false);
    expect(solved(hole, [])).toBe(false);
    expect(solved(hole, [hole.want, hole.want])).toBe(false);
  });

  it('compares lists and objects by value and by order', () => {
    const shape = makeHole(seeded(5), 'shape') as { want: Record<string, unknown> } & typeof hole;
    const flipped = Object.fromEntries(Object.entries(shape.want).reverse());
    expect(solved(shape, [shape.want])).toBe(true);
    expect(solved(shape, [flipped])).toBe(false);
  });
});

describe('the verdict', () => {
  const hole = makeHole(seeded(11), 'field');

  it('says nothing until something is typed', () => {
    expect(verdictOf(hole, '   ', [], null)).toBe('empty');
  });

  it('is the engine error when the expression does not compile', () => {
    expect(verdictOf(hole, '.name|', [], 'unexpected end')).toBe('error');
  });

  it('is wrong when it runs and answers something else', () => {
    expect(verdictOf(hole, '.id', ['u-1'], null)).toBe('wrong');
  });

  it('counts strokes against par', () => {
    const at = (n: number) => 'x'.repeat(n);
    expect(verdictOf(hole, at(hole.par), [hole.want], null)).toBe('par');
    expect(verdictOf(hole, at(hole.par + 2), [hole.want], null)).toBe('over');
    expect(verdictOf(hole, at(hole.par - 1), [hole.want], null)).toBe('under');
  });

  it('refuses an answer that does not read the input', () => {
    const numeric = makeHole(seeded(19), 'length');
    expect(verdictOf(numeric, `${numeric.want}`, [numeric.want], null, [numeric.want])).toBe('constant');
    expect(verdictOf(numeric, '.rows|length', [numeric.want], null, [numeric.decoyWant])).not.toBe('constant');
  });
});

describe('reading the input', () => {
  it('is what a second run over other data proves', () => {
    const hole = makeHole(seeded(21), 'sum');
    expect(reads(hole, [hole.want])).toBe(false);
    expect(reads(hole, [hole.decoyWant])).toBe(true);
    expect(reads(hole, [])).toBe(false);
  });

  it('gives every hole other data whose answer differs', () => {
    for (const kind of KINDS) {
      for (let seed = 1; seed <= 10; seed++) {
        const hole = makeHole(seeded(seed), kind);
        expect(JSON.stringify(hole.decoyWant) !== JSON.stringify(hole.want), `${kind}/${seed}`).toBe(true);
      }
    }
  });
});

describe('the round', () => {
  const hole = makeHole(seeded(13), 'sum');

  it('counts a solved hole, its strokes and its par', () => {
    const round = scored(emptyRound(), hole, 12, 8);
    expect(round).toMatchObject({ holes: 1, solved: 1, missed: 0, strokes: 12, par: hole.par });
  });

  it('keeps the longest streak of solved holes as the best', () => {
    let round = scored(scored(emptyRound(), hole, 5, 3), hole, 5, 3);
    expect(round.best).toBe(2);
    round = missed(round);
    expect(round).toMatchObject({ holes: 3, solved: 2, missed: 1, best: 2 });
  });

  it('does not score a hole that ran out of time', () => {
    const round = missed(emptyRound());
    expect(round.strokes).toBe(0);
    expect(round.par).toBe(0);
  });
});

describe('the clock', () => {
  it('gives a harder hole more time', () => {
    const easy = makeHole(seeded(2), 'field');
    const hard = makeHole(seeded(2), 'select');
    expect(secondsFor(easy)).toBe(TIER[1]);
    expect(secondsFor(hard)).toBe(TIER[3]);
    expect(secondsFor(hard)).toBeGreaterThan(secondsFor(easy));
  });

  it('carries what was left of the last hole into the next', () => {
    const hole = makeHole(seeded(4), 'field');
    expect(startClock(hole, 0)).toBe(secondsFor(hole));
    expect(startClock(hole, 7)).toBe(secondsFor(hole) + 7);
  });

  it('caps what can be carried, so a good run does not become an idle one', () => {
    const hole = makeHole(seeded(4), 'field');
    expect(startClock(hole, 999)).toBe(secondsFor(hole) + MAX_BANK);
    expect(startClock(hole, -5)).toBe(secondsFor(hole));
  });

  it('banks the time a solved hole had left, and a missed hole banks nothing', () => {
    const hole = makeHole(seeded(6), 'sum');
    expect(scored(emptyRound(), hole, 10, 12).banked).toBe(12);
    expect(scored(emptyRound(), hole, 10, 999).banked).toBe(MAX_BANK);
    expect(missed(scored(emptyRound(), hole, 10, 12)).banked).toBe(0);
  });

  it('stops at zero', () => {
    expect(clockLabel(12)).toBe('12s');
    expect(clockLabel(-3)).toBe('0s');
    expect(running(1)).toBe(true);
    expect(running(0)).toBe(false);
  });
});

describe('how a score reads', () => {
  const hole = makeHole(seeded(17), 'keys');
  it('says par, under and over in words', () => {
    expect(scoreLine(hole, hole.par)).toContain('par');
    expect(scoreLine(hole, hole.par - 2)).toContain('2 under par');
    expect(scoreLine(hole, hole.par + 3)).toContain('3 over par');
  });
});
