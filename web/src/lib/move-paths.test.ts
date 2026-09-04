import { describe, it, expect } from 'vitest';
import { movedPath, labelFor } from './move-paths';

describe('following a file that moved', () => {
  it('follows the file itself', () => {
    expect(movedPath('auth/login.gctf', 'auth/login.gctf', 'auth/signin.gctf')).toBe('auth/signin.gctf');
  });

  it('follows everything under a folder that moved', () => {
    expect(movedPath('auth/login.gctf', 'auth', 'identity')).toBe('identity/login.gctf');
    expect(movedPath('auth/deep/one.gctf', 'auth', 'identity')).toBe('identity/deep/one.gctf');
  });

  it('leaves a path that only looks similar alone', () => {
    expect(movedPath('authority/login.gctf', 'auth', 'identity')).toBeNull();
    expect(movedPath('feed/crud.gctf', 'auth/login.gctf', 'auth/signin.gctf')).toBeNull();
  });

  it('names a tab by the file', () => {
    expect(labelFor('identity/deep/one.gctf')).toBe('one.gctf');
    expect(labelFor('one.gctf')).toBe('one.gctf');
  });
});
