import { describe, expect, it } from 'vitest';
import { statusAssert, statusUnchecked } from './http-expect';

describe('whether an HTTP file checks the status', () => {
  it('is unchecked when no line mentions it', () => {
    expect(statusUnchecked([])).toBe(true);
    expect(statusUnchecked(['.name == "Ada"'])).toBe(true);
  });

  it('is checked whichever way the line is written', () => {
    expect(statusUnchecked(['@status() == 200'])).toBe(false);
    expect(statusUnchecked(['.name == "Ada"', '@status() >= 400'])).toBe(false);
    expect(statusUnchecked(['@status()|tostring == "200"'])).toBe(false);
  });
});

describe('the line that checks it', () => {
  it('takes the code the call answered', () => {
    expect(statusAssert(201)).toBe('@status() == 201');
    expect(statusAssert(404)).toBe('@status() == 404');
  });

  it('is 200 before there has been a call, or when the code is not one', () => {
    expect(statusAssert(null)).toBe('@status() == 200');
    expect(statusAssert(undefined)).toBe('@status() == 200');
    expect(statusAssert(0)).toBe('@status() == 200');
    expect(statusAssert(700)).toBe('@status() == 200');
  });
});
