import { describe, it, expect } from 'vitest';
import { planFacts, planHeadline, stepAsserts, stepSkips } from './plan-headline';
import type { DocumentSummary } from './types';

function doc(over: Partial<DocumentSummary> = {}): DocumentSummary {
  return {
    index: 0, endpoint: 'pkg.Svc/M', kind: 'unary', address: 'localhost:4770',
    address_source: 'section', headers: {}, bodies: ['{}'], asserts: [], extracts: {},
    options: {}, tls: {}, proto: {}, produces: [], consumes: [],
    ...over,
  };
}

describe('planFacts', () => {
  it('adds up what the file checks and what it passes on', () => {
    const facts = planFacts(
      [
        doc({ asserts: ['.ok == true'], extracts: { token: '.token' } }),
        doc({ index: 1, asserts: ['.id != ""', '.name != ""'] }),
      ],
      [false, false],
    );
    expect(facts).toMatchObject({
      steps: 2, target: 'localhost:4770', targets: 1, asserts: 3, variables: 1,
      expectsError: false, streaming: false,
    });
  });

  it('refuses to name one target when the steps disagree', () => {
    const facts = planFacts([doc(), doc({ index: 1, address: 'other:443' })], [false, false]);
    expect(facts.target).toBeNull();
    expect(facts.targets).toBe(2);
  });

  it('notices streaming and an expected error', () => {
    const facts = planFacts([doc({ kind: 'server' })], [true]);
    expect(facts.streaming).toBe(true);
    expect(facts.expectsError).toBe(true);
  });
});

describe('planHeadline', () => {
  it('reads as a sentence, shortest first', () => {
    expect(planHeadline(planFacts([doc({ asserts: ['a'] })], [false])))
      .toBe('1 step · localhost:4770 · 1 assert');
  });

  it('names every target count it cannot collapse', () => {
    const line = planHeadline(planFacts([doc(), doc({ index: 1, address: 'b:1' })], [false, true]));
    expect(line).toBe('2 steps · 2 targets · expects an error');
  });

  it('counts a RESPONSE block as the check it is', () => {
    expect(planHeadline(planFacts([doc()], [false], [1])))
      .toBe('1 step · localhost:4770 · 1 expected response');
  });

  it('says nothing checked only when nothing checks anything', () => {
    expect(planHeadline(planFacts([doc()], [false], [0]))).toContain('nothing checked');
    expect(planHeadline(planFacts([doc()], [false], [2]))).not.toContain('nothing checked');
    expect(planHeadline(planFacts([doc({ asserts: ['a'] })], [false]))).not.toContain('nothing checked');
    expect(planHeadline(planFacts([doc()], [true]))).not.toContain('nothing checked');
  });

  it('says variables and streaming only when there are any', () => {
    const line = planHeadline(planFacts([doc({ kind: 'bidi', extracts: { a: '.a', b: '.b' } })], [false]));
    expect(line).toContain('2 variables');
    expect(line).toContain('streaming');
  });
});

describe('what one step of a plan checks', () => {
  it('counts the checks, not the sections holding them', () => {
    expect(stepAsserts([{ assertions: ['@status() == 200', '.name == "Ada"'] }])).toBe(2);
    expect(stepAsserts([{ assertions: ['a'] }, { assertions: ['b', 'c'] }])).toBe(3);
  });

  it('is nothing when a step checks nothing', () => {
    expect(stepAsserts([])).toBe(0);
    expect(stepAsserts([{}])).toBe(0);
    expect(stepAsserts([{ assertions: [] }])).toBe(0);
  });
});

describe('the sections a run walks past', () => {
  it('names them, and numbers a message only when there are several', () => {
    expect(stepSkips({ requests: [{ skipped: true }] })).toEqual(['REQUEST']);
    expect(stepSkips({ requests: [{ skipped: false }, { skipped: true }] })).toEqual(['REQUEST 2']);
    expect(stepSkips({
      expectations: [{ skipped: true, expectation_type: 'response' }],
      assertions: [{ skipped: true }],
      extractions: [{ skipped: true }],
    })).toEqual(['RESPONSE', 'ASSERTS', 'EXTRACT']);
    expect(stepSkips({ expectations: [{ skipped: true, expectation_type: 'error' }] })).toEqual(['ERROR']);
  });

  it('is nothing for a file that skips none of them', () => {
    expect(stepSkips({ requests: [{}], assertions: [{}], expectations: [{}] })).toEqual([]);
  });

  it('leaves a skipped block out of the check count', () => {
    expect(stepAsserts([{ assertions: ['.a == 1', '.b == 2'] }, { assertions: ['.c == 3'], skipped: true }]))
      .toBe(2);
  });
});

describe('a headline for a file that skips its checks', () => {
  it('counts what the plan says runs, not what the file holds', () => {
    const documents = [doc({ asserts: ['.a == 1'], extracts: { id: '.id' } })];
    expect(planFacts(documents, [false], [0], [{ asserts: 0, variables: 0 }]).asserts).toBe(0);
    expect(planFacts(documents, [false], [0], [{ asserts: 0, variables: 0 }]).variables).toBe(0);
    expect(planHeadline(planFacts(documents, [false], [0], [{ asserts: 0, variables: 0 }])))
      .toContain('nothing checked');
  });

  it('falls back to the file when no plan came with it', () => {
    expect(planFacts([doc({ asserts: ['.a == 1'] })], [false]).asserts).toBe(1);
  });
});
