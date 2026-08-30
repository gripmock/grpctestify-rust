import { describe, expect, it } from 'vitest';
import { copiedNote, nextCopyName } from './duplicate-name';

describe('the name a copy takes', () => {
  it('is the file again, numbered, in the same folder', () => {
    expect(nextCopyName('auth/login.gctf', ['auth/login.gctf'])).toBe('auth/login-2.gctf');
    expect(nextCopyName('probe.httf', ['probe.httf'])).toBe('probe-2.httf');
  });

  it('takes the first number nothing else has', () => {
    expect(nextCopyName('auth/login.gctf', ['auth/login.gctf', 'auth/login-2.gctf']))
      .toBe('auth/login-3.gctf');
  });

  it('counts on from a name that is already numbered', () => {
    expect(nextCopyName('auth/login-2.gctf', ['auth/login.gctf', 'auth/login-2.gctf']))
      .toBe('auth/login-3.gctf');
  });

  it('keeps a name that has no extension whole', () => {
    expect(nextCopyName('notes', ['notes'])).toBe('notes-2');
  });

  it('keeps the extension of a file whose name has dots in it', () => {
    expect(nextCopyName('a/my.test.gctf', ['a/my.test.gctf'])).toBe('a/my.test-2.gctf');
  });
});

describe('what is said after making one', () => {
  it('names the copy and what it came from', () => {
    expect(copiedNote('auth/login-2.gctf', 'auth/login.gctf', false))
      .toBe('auth/login-2.gctf — a copy of login.gctf');
  });

  it('says the unsaved edits are not in it', () => {
    expect(copiedNote('auth/login-2.gctf', 'auth/login.gctf', true))
      .toContain('as it is on disk, without the unsaved edits');
  });
});
