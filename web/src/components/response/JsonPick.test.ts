import { describe, it, expect } from 'vitest';
import { childPath } from './JsonPick';

describe('childPath', () => {
  it('uses the bare form for identifier keys', () => {
    expect(childPath('', 'auth')).toBe('.auth');
    expect(childPath('.auth', 'user')).toBe('.auth.user');
    expect(childPath('.a', '_b0')).toBe('.a._b0');
  });

  it('brackets a key jq cannot take bare — with no dot before the bracket', () => {
    expect(childPath('.user', 'user-id')).toBe('.user["user-id"]');
    expect(childPath('', 'user-id')).toBe('.["user-id"]');
    expect(childPath('.a', 'b.c')).toBe('.a["b.c"]');
    expect(childPath('.a', '2fa')).toBe('.a["2fa"]');
  });

  it('escapes quotes and backslashes inside the key', () => {
    expect(childPath('.a', 'say "hi"')).toBe('.a["say \\"hi\\""]');
    expect(childPath('.a', 'back\\slash')).toBe('.a["back\\\\slash"]');
  });
});
