import { describe, it, expect } from 'vitest';
import { matchesQuery } from '../../lib/method-search';

const SAY_HELLO = 'helloworld.Greeter/SayHello';

describe('matchesQuery', () => {
  it('matches by method name', () => {
    expect(matchesQuery(SAY_HELLO, 'SayHello')).toBe(true);
  });

  it('matches by service name without the package', () => {
    expect(matchesQuery(SAY_HELLO, 'Greeter')).toBe(true);
  });

  it('matches by package', () => {
    expect(matchesQuery(SAY_HELLO, 'helloworld')).toBe(true);
  });

  it('matches multiple tokens in any order', () => {
    expect(matchesQuery(SAY_HELLO, 'greeter hello')).toBe(true);
    expect(matchesQuery(SAY_HELLO, 'hello greeter')).toBe(true);
  });

  it('is case insensitive', () => {
    expect(matchesQuery(SAY_HELLO, 'GREETER/sayhello')).toBe(true);
  });

  it('rejects a token that appears nowhere', () => {
    expect(matchesQuery(SAY_HELLO, 'greeter missing')).toBe(false);
  });

  it('treats a blank query as matching', () => {
    expect(matchesQuery(SAY_HELLO, '   ')).toBe(true);
  });
});
