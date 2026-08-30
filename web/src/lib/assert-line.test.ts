import { describe, it, expect } from 'vitest';
import { assertWhy, groupByStep, isBlock, stepHeading, stepOfLine, stepPhrase, takeApart } from './assert-line';

describe('assertWhy', () => {
  it('drops the operator equality already states', () => {
    expect(assertWhy({ expected: '== false', actual: 'true', message: 'Assertion failed: .ok == false' }))
      .toEqual({ expected: 'false', actual: 'true', message: null, hint: null });
  });

  it('keeps an operator that is not equality', () => {
    expect(assertWhy({ expected: '>= 3', actual: '1' }))
      .toEqual({ expected: '>= 3', actual: '1', message: null, hint: null });
  });

  it('falls back to the message when there is no pair', () => {
    expect(assertWhy({ message: 'Invalid regex: (' }))
      .toEqual({ expected: null, actual: null, message: 'Invalid regex: (', hint: null });
  });

  it('says nothing when there is nothing to say', () => {
    expect(assertWhy({})).toBeNull();
    expect(assertWhy({ expected: '  ', actual: '', message: '' })).toBeNull();
  });

  it('shows a lone actual', () => {
    expect(assertWhy({ actual: 'null' })).toEqual({ expected: null, actual: 'null', message: null, hint: null });
  });
});

describe('a whole-message comparison', () => {
  it('is a block when either side spans lines', () => {
    expect(isBlock(assertWhy({ expected: '{\n "a": 1\n}', actual: '{\n "a": 2\n}' }))).toBe(true);
    expect(isBlock(assertWhy({ expected: '== 3', actual: '4' }))).toBe(false);
    expect(isBlock(null)).toBe(false);
  });
});

describe('which step a check belongs to', () => {
  const steps = [
    { index: 0, endpoint: 'GET /v1/users', start_line: 0, end_line: 12 },
    { index: 1, endpoint: 'GET /v1/users/{{user}}', start_line: 12, end_line: 18 },
  ];

  it('reads a line the way the parser records one', () => {
    expect(stepOfLine(steps, 8)?.index).toBe(0);
    expect(stepOfLine(steps, 12)?.index).toBe(0);
    expect(stepOfLine(steps, 13)?.index).toBe(1);
    expect(stepOfLine(steps, 18)?.index).toBe(1);
    expect(stepOfLine(steps, 99)).toBeNull();
  });

  it('cuts the list where the step changes', () => {
    const groups = groupByStep([{ line: 8 }, { line: 17 }, { line: 18 }], steps);
    expect(groups).toHaveLength(2);
    expect(groups[0].step?.index).toBe(0);
    expect(groups[1].checks).toHaveLength(2);
  });

  it('labels nothing when there is one step', () => {
    const groups = groupByStep([{ line: 3 }, { line: 4 }], [steps[0]]);
    expect(groups).toEqual([{ step: null, checks: [{ line: 3 }, { line: 4 }] }]);
  });
});

describe(`the heading over a step's checks`, () => {
  it('names the request as it was dialled', () => {
    expect(stepHeading('GET /v1/users/{{user}}', [
      { endpoint: 'GET /v1/users/7' },
      { endpoint: 'GET /v1/users/7' },
    ])).toBe('GET /v1/users/7');
  });

  it(`keeps the file's own words when the checks say nothing`, () => {
    expect(stepHeading('GET /v1/users/{{user}}', [{ passed: true } as never])).toBe('GET /v1/users/{{user}}');
    expect(stepHeading('a.B/C', [])).toBe('a.B/C');
  });

  it(`keeps the file's own words when they disagree`, () => {
    expect(stepHeading('GET /v1/users/{{user}}', [
      { endpoint: 'GET /v1/users/7' },
      { endpoint: 'GET /v1/users/8' },
    ])).toBe('GET /v1/users/{{user}}');
  });
});

describe('which step a tool writes into', () => {
  it('names the step of a chain', () => {
    expect(stepPhrase(3, 1)).toBe('step 2');
    expect(stepPhrase(2, 0)).toBe('step 1');
  });

  it('says nothing where the file is one document', () => {
    expect(stepPhrase(1, 0)).toBe('');
    expect(stepPhrase(0, 0)).toBe('');
  });
});

describe('the remedy under a failed check', () => {
  it('carries the hint beside the two values', () => {
    const why = assertWhy({
      expected: '== 3600',
      actual: '"3600"',
      hint: 'the answer holds it as a string; compare with `.expires_in:number`',
    });
    expect(why?.expected).toBe('3600');
    expect(why?.hint).toContain(':number');
  });

  it('is worth showing on its own when there is no pair', () => {
    expect(assertWhy({ hint: 'try a cast' })?.hint).toBe('try a cast');
  });

  it('says nothing when the check carries none', () => {
    expect(assertWhy({ expected: '== 1', actual: '2' })?.hint).toBeNull();
    expect(assertWhy({})).toBeNull();
  });
});

describe('the message under a jq failure', () => {
  it('does not repeat the expression printed above it', () => {
    const why = assertWhy({
      message: 'JQ assertion evaluated to falsy value false: .message | test("Hi")',
      expression: '.message | test("Hi")',
    });
    expect(why?.message).toBe('JQ assertion evaluated to falsy value false');
  });

  it('keeps a message that ends in something else', () => {
    const why = assertWhy({ message: 'jq error: unexpected token', expression: '.a' });
    expect(why?.message).toBe('jq error: unexpected token');
  });
});

describe('what the drawer can take apart', () => {
  it('takes a jq filter', () => {
    expect(takeApart('.message | test("Hi")')).toBe(true);
    expect(takeApart('  .items | length >= 2 ')).toBe(true);
  });

  it('refuses what it cannot run', () => {
    expect(takeApart('@status() == 200')).toBe(false);
    expect(takeApart('{\n  "a": 1\n}')).toBe(false);
    expect(takeApart('RESPONSE')).toBe(false);
  });
});
