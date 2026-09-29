export interface Hole {
  kind: string;
  ask: string;
  input: unknown;
  want: unknown;
  par: number;
  hard: 1 | 2 | 3;
  decoy: unknown;
  decoyWant: unknown;
}

export const TIER: Record<1 | 2 | 3, number> = { 1: 20, 2: 30, 3: 45 };
export const MAX_BANK = 30;

export function secondsFor(hole: Hole): number {
  return TIER[hole.hard];
}

export function startClock(hole: Hole, banked: number): number {
  return secondsFor(hole) + Math.max(0, Math.min(MAX_BANK, Math.floor(banked)));
}

export type Rng = () => number;

function one<T>(rng: Rng, list: readonly T[]): T {
  return list[Math.min(list.length - 1, Math.floor(rng() * list.length))];
}

function some<T>(rng: Rng, list: readonly T[], count: number): T[] {
  const pool = [...list];
  const out: T[] = [];
  for (let i = 0; i < count && pool.length > 0; i++) {
    out.push(pool.splice(Math.min(pool.length - 1, Math.floor(rng() * pool.length)), 1)[0]);
  }
  return out;
}

function between(rng: Rng, low: number, high: number): number {
  return low + Math.floor(rng() * (high - low + 1));
}

const NAMES = ['Ada', 'Grace', 'Alan', 'Edsger', 'Barbara', 'Ken', 'Linus', 'Margaret'] as const;
const CITIES = ['London', 'Berlin', 'Lisbon', 'Oslo', 'Kyoto', 'Vienna'] as const;
const FIELDS = ['name', 'title', 'label', 'handle'] as const;
const HOLDERS = ['user', 'account', 'profile', 'owner'] as const;
const LISTS = ['items', 'rows', 'entries', 'records'] as const;
const STATES = ['queued', 'sent', 'failed', 'retried', 'done'] as const;

type Build = (rng: Rng) => { ask: string; input: unknown; want: unknown; ref: string };

const HARD: Record<string, 1 | 2 | 3> = {
  field: 1,
  nested: 1,
  length: 1,
  keys: 1,
  index: 2,
  pluck: 2,
  last: 2,
  join: 2,
  shape: 2,
  sum: 3,
  max: 3,
  select: 3,
  sorted: 3,
  count: 3,
};

const TEMPLATES: Record<string, Build> = {
  field: rng => {
    const field = one(rng, FIELDS);
    const value = one(rng, NAMES);
    return {
      ask: `the ${field}`,
      input: { id: `u-${between(rng, 1, 99)}`, [field]: value, active: rng() < 0.5 },
      want: value,
      ref: `.${field}`,
    };
  },

  nested: rng => {
    const holder = one(rng, HOLDERS);
    const city = one(rng, CITIES);
    return {
      ask: `the city of the ${holder}'s address`,
      input: { [holder]: { name: one(rng, NAMES), address: { city, zip: `Z${between(rng, 10, 99)}` } } },
      want: city,
      ref: `.${holder}.address.city`,
    };
  },

  index: rng => {
    const list = one(rng, LISTS);
    const size = between(rng, 3, 5);
    const at = between(rng, 1, size - 1);
    const ids = Array.from({ length: size }, () => between(rng, 10, 99));
    return {
      ask: `the id at position ${at + 1}`,
      input: { [list]: ids.map(id => ({ id })) },
      want: ids[at],
      ref: `.${list}[${at}].id`,
    };
  },

  length: rng => {
    const list = one(rng, LISTS);
    const size = between(rng, 3, 8);
    return {
      ask: `how many ${list} there are`,
      input: { [list]: Array.from({ length: size }, (_, i) => i + 1) },
      want: size,
      ref: `.${list}|length`,
    };
  },

  pluck: rng => {
    const field = one(rng, FIELDS);
    const names = some(rng, NAMES, between(rng, 2, 4));
    return {
      ask: `every ${field}, as a list`,
      input: { users: names.map(n => ({ [field]: n, id: between(rng, 1, 99) })) },
      want: names,
      ref: `[.users[].${field}]`,
    };
  },

  select: rng => {
    const names = some(rng, NAMES, 4);
    const active = names.map(() => rng() < 0.5);
    if (!active.includes(true)) active[0] = true;
    return {
      ask: 'the names of the active users',
      input: { users: names.map((name, i) => ({ name, active: active[i] })) },
      want: names.filter((_, i) => active[i]),
      ref: '[.users[]|select(.active).name]',
    };
  },

  sum: rng => {
    const amounts = Array.from({ length: between(rng, 3, 5) }, () => between(rng, 1, 40));
    return {
      ask: 'the total of the amounts',
      input: { lines: amounts.map(amount => ({ amount })) },
      want: amounts.reduce((a, b) => a + b, 0),
      ref: '[.lines[].amount]|add',
    };
  },

  max: rng => {
    const amounts = some(rng, Array.from({ length: 40 }, (_, i) => i + 1), between(rng, 3, 5));
    return {
      ask: 'the largest amount',
      input: { lines: amounts.map(amount => ({ amount })) },
      want: Math.max(...amounts),
      ref: '[.lines[].amount]|max',
    };
  },

  keys: rng => {
    const chosen = some(rng, [...FIELDS, ...HOLDERS, 'id', 'active'], between(rng, 3, 4));
    const input: Record<string, number> = {};
    chosen.forEach((key, i) => { input[key] = i + 1; });
    return {
      ask: 'the keys of the object, sorted',
      input,
      want: [...chosen].sort(),
      ref: 'keys',
    };
  },

  last: rng => {
    const states = Array.from({ length: between(rng, 3, 5) }, () => one(rng, STATES));
    return {
      ask: 'the status of the last event',
      input: { events: states.map((status, i) => ({ at: i + 1, status })) },
      want: states[states.length - 1],
      ref: '.events[-1].status',
    };
  },

  shape: rng => {
    const name = one(rng, NAMES);
    const id = `u-${between(rng, 1, 99)}`;
    return {
      ask: 'an object of id and name only',
      input: { id, name, secret: 'x', active: rng() < 0.5 },
      want: { id, name },
      ref: '{id,name}',
    };
  },

  join: rng => {
    const tags = some(rng, ['api', 'auth', 'slow', 'flaky', 'smoke', 'wip'], between(rng, 2, 4));
    return {
      ask: 'the tags joined by a comma',
      input: { tags },
      want: tags.join(','),
      ref: '.tags|join(",")',
    };
  },

  sorted: rng => {
    const people = some(rng, NAMES, 4).map(name => ({ name, age: between(rng, 20, 70) }));
    return {
      ask: 'the names, youngest first',
      input: { users: people },
      want: [...people].sort((a, b) => a.age - b.age).map(p => p.name),
      ref: '[.users|sort_by(.age)[].name]',
    };
  },

  count: rng => {
    const names = some(rng, NAMES, 5);
    const failed = names.map(() => rng() < 0.5);
    return {
      ask: 'how many runs failed',
      input: { runs: names.map((name, i) => ({ name, failed: failed[i] })) },
      want: failed.filter(Boolean).length,
      ref: '[.runs[]|select(.failed)]|length',
    };
  },
};

export const KINDS = Object.keys(TEMPLATES);

export function makeHole(rng: Rng, kind = one(rng, KINDS)): Hole {
  const built = TEMPLATES[kind](rng);
  let decoy = TEMPLATES[kind](rng);
  for (let tries = 0; tries < 12 && same(decoy.want, built.want); tries++) {
    decoy = TEMPLATES[kind](rng);
  }
  return {
    kind,
    ask: built.ask,
    input: built.input,
    want: built.want,
    par: built.ref.length,
    hard: HARD[kind] ?? 2,
    decoy: decoy.input,
    decoyWant: decoy.want,
  };
}

export function reads(hole: Hole, decoyOutputs: unknown[]): boolean {
  return decoyOutputs.length === 1 && !same(decoyOutputs[0], hole.want);
}

export function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function solved(hole: Hole, outputs: unknown[]): boolean {
  return outputs.length === 1 && same(outputs[0], hole.want);
}

export type Verdict = 'empty' | 'error' | 'wrong' | 'constant' | 'over' | 'par' | 'under';

export function verdictOf(
  hole: Hole,
  expr: string,
  outputs: unknown[],
  error: string | null,
  decoyOutputs: unknown[] | null = null,
): Verdict {
  if (!expr.trim()) return 'empty';
  if (error) return 'error';
  if (!solved(hole, outputs)) return 'wrong';
  if (decoyOutputs !== null && !reads(hole, decoyOutputs)) return 'constant';
  const strokes = expr.trim().length;
  if (strokes < hole.par) return 'under';
  return strokes === hole.par ? 'par' : 'over';
}

export function scoreLine(hole: Hole, strokes: number): string {
  const over = strokes - hole.par;
  if (over === 0) return `par — ${strokes}`;
  if (over < 0) return `${-over} under par — ${strokes}`;
  return `${over} over par — ${strokes}`;
}

export interface Round {
  holes: number;
  solved: number;
  missed: number;
  strokes: number;
  par: number;
  best: number;
  banked: number;
}

export const emptyRound = (best = 0): Round =>
  ({ holes: 0, solved: 0, missed: 0, strokes: 0, par: 0, best, banked: 0 });

export function scored(round: Round, hole: Hole, strokes: number, left: number): Round {
  const next = {
    ...round,
    holes: round.holes + 1,
    solved: round.solved + 1,
    strokes: round.strokes + strokes,
    par: round.par + hole.par,
    banked: Math.max(0, Math.min(MAX_BANK, Math.floor(left))),
  };
  return { ...next, best: Math.max(next.best, next.solved) };
}

export function missed(round: Round): Round {
  return { ...round, holes: round.holes + 1, missed: round.missed + 1, banked: 0 };
}

export function clockLabel(left: number): string {
  return `${Math.max(0, left)}s`;
}

export function running(left: number): boolean {
  return left > 0;
}
