import { describe, expect, it } from 'vitest';
import { chainDiagram, diagramLayout, fit, groupsOf } from './chain-diagram';
import type { DocumentSummary } from './types';

const doc = (over: Partial<DocumentSummary>): DocumentSummary => ({
  index: 0,
  endpoint: 'GET /v1/users',
  kind: 'unary',
  address: 'http://127.0.0.1:8899',
  address_source: 'section',
  headers: {},
  bodies: [],
  asserts: [],
  extracts: {},
  options: {},
  tls: {},
  proto: {},
  produces: [],
  consumes: [],
  ...over,
} as DocumentSummary);

const plain = { error_expected: false, total_responses: 0 };

describe('the chain as a drawing', () => {
  it('is one band per step, in the order they run', () => {
    const model = chainDiagram(
      [doc({ endpoint: 'GET /a' }), doc({ endpoint: 'GET /b', address: '' })],
      [plain, plain],
    );
    expect(model.steps.map(s => [s.index, s.request])).toEqual([[1, 'GET /a'], [2, 'GET /b']]);
    expect(model.server).toBe('http://127.0.0.1:8899');
  });

  it('counts a step by what it checks, ASSERTS and RESPONSE alike', () => {
    const [one, two, none] = chainDiagram(
      [doc({ asserts: ['a'] }), doc({ asserts: ['a', 'b'] }), doc({})],
      [plain, { error_expected: false, total_responses: 1 }, plain],
    ).steps;
    expect(one.response).toBe('the answer · 1 check');
    expect(two.response).toBe('the answer · 3 checks');
    expect(none.response).toBe('the answer, unchecked');
  });

  it('says when the answer the file wants is an error', () => {
    const [step] = chainDiagram([doc({ asserts: ['a'] })], [{ error_expected: true, total_responses: 0 }]).steps;
    expect(step.response).toBe('an error · 1 check');
  });

  it('marks what a step binds for the steps after it', () => {
    const [step] = chainDiagram([doc({ extracts: { user: '.id', token: '.t' } })], [plain]).steps;
    expect(step.binds).toEqual(['user', 'token']);
  });

  it('knows a stream from a single message', () => {
    const [step] = chainDiagram([doc({ kind: 'server' })], [plain]).steps;
    expect(step.streaming).toBe(true);
  });
});

describe('where the drawing puts things', () => {
  it('reserves room for the note only where there is one', () => {
    const model = chainDiagram([doc({ extracts: { user: '.id' } }), doc({})], [plain, plain]);
    const { height, steps } = diagramLayout(model);
    expect(steps[0].note).not.toBeNull();
    expect(steps[1].note).toBeNull();
    expect(steps[0].height).toBeGreaterThan(steps[1].height);
    expect(height).toBeGreaterThan(steps[1].y + steps[1].height);
  });

  it('stacks the steps without overlapping them', () => {
    const model = chainDiagram([doc({}), doc({}), doc({})], [plain, plain, plain]);
    const { steps } = diagramLayout(model);
    expect(steps[1].y).toBe(steps[0].y + steps[0].height);
    expect(steps[2].y).toBe(steps[1].y + steps[1].height);
    expect(steps[0].request).toBeLessThan(steps[0].response);
  });
});

describe('a label too long for the lane', () => {
  it('is cut where it stops fitting', () => {
    expect(fit('short')).toBe('short');
    expect(fit('abcdefghij', 5)).toBe('abcd…');
  });
});

describe('the steps that go out together', () => {
  const steps = (...flags: boolean[]) => flags.map(parallel => ({ parallel }));

  it('is a run of consecutive marked steps', () => {
    expect(groupsOf(steps(false, true, true, false, true, true, true)))
      .toEqual([{ start: 1, end: 2 }, { start: 4, end: 6 }]);
  });

  it('is not one step on its own', () => {
    expect(groupsOf(steps(false, true, false))).toEqual([]);
  });

  it('is nothing at all in a chain that marks none', () => {
    expect(groupsOf(steps(false, false))).toEqual([]);
  });
});

describe('a step that skips its checks', () => {
  it('is drawn as one that checks nothing', () => {
    const [step] = chainDiagram(
      [doc({ asserts: ['a', 'b'], extracts: { id: '.id' } })],
      [{ error_expected: false, total_responses: 1, running: { checks: 0, binds: [] } }],
    ).steps;
    expect(step.response).toContain('unchecked');
    expect(step.binds).toEqual([]);
  });

  it('still reads the file when no plan came with it', () => {
    const [step] = chainDiagram([doc({ asserts: ['a'] })], [{ error_expected: false, total_responses: 0 }]).steps;
    expect(step.response).toContain('1 check');
  });
});
