import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SRC = import.meta.dirname;
const CSS = [
  join(SRC, '..', 'app.css'),
  join(SRC, '..', '..', 'luvo', 'controls.css'),
  join(SRC, '..', '..', 'luvo', 'base.css'),
].map(f => readFileSync(f, 'utf8')).join('\n');

interface Family {
  base: string;
  from: string;
  styled: string[];
  plain?: string[];
}

const FAMILIES: Family[] = [
  {
    base: '.rail-dot', from: "stepMarks() in lib/jobs.ts",
    styled: ['is-pass', 'is-fail', 'is-skip'], plain: ['is-none'],
  },
  {
    base: '.problem', from: "severityLabel() in lib/problems.ts",
    styled: ['is-error', 'is-warning', 'is-info'],
  },
  {
    base: '.history-payload', from: "callSummary() in lib/history-group.ts",
    styled: ['is-response', 'is-error'], plain: ['is-request'],
  },
  {
    base: '.extract-value', from: 'ExtractValue in lib/extract-preview.ts',
    styled: ['is-value', 'is-many', 'is-none', 'is-null', 'is-error'],
  },
  {
    base: '.kvrow', from: 'checkMetadataKey() in lib/metadata.ts',
    styled: ['is-bad', 'is-note'],
  },
  {
    base: '.summary .count', from: 'RunBar’s pass/fail/skip counts',
    styled: ['is-ok', 'is-fail', 'is-skip'],
  },
  {
    base: '', from: 'familyOf() in lib/tree.ts',
    styled: ['is-gctf', 'is-httf', 'is-apif', 'is-unknown'],
  },
  {
    base: '.toast', from: "ToastContext's kind",
    styled: ['is-ok', 'is-fail', 'is-info'],
  },
];

describe('every state a component can write has a rule', () => {
  it.each(FAMILIES.flatMap(f => f.styled.map(state => [f.base, state, f.from] as const)))(
    '%s.%s — %s',
    (base, state, _from) => {
      const selector = `${base}.${state}`.replace(/([.*+?^${}()|[\]\\])/g, '\\$1');
      expect(new RegExp(selector).test(CSS), `${base}${state} has no rule`).toBe(true);
    },
  );

  it('and the plain states are named rather than forgotten', () => {
    const plain = FAMILIES.flatMap(f => (f.plain ?? []).map(state => `${f.base}.${state}`));
    expect(plain).toEqual(['.rail-dot.is-none', '.history-payload.is-request']);
  });
});
