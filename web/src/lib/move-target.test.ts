import { describe, expect, it } from 'vitest';
import { createRefusal, moveRefusal } from './move-target';

const paths = ['auth/login.gctf', 'auth/logout.gctf', 'billing/charge.gctf', 'shared/schema.bin'];

describe('what a move is refused for', () => {
  it('takes a free path', () => {
    expect(moveRefusal('auth/login.gctf', 'auth/signin.gctf', paths)).toBeNull();
    expect(moveRefusal('auth/login.gctf', 'billing/login.gctf', paths)).toBeNull();
  });

  it('refuses a path something already holds', () => {
    expect(moveRefusal('auth/login.gctf', 'auth/logout.gctf', paths))
      .toBe('auth/logout.gctf already exists — pick another name.');
  });

  it('refuses a name a folder holds', () => {
    expect(moveRefusal('auth/login.gctf', 'billing', paths))
      .toBe('billing is a folder — pick another name.');
  });

  it('refuses a path that leaves the collection', () => {
    expect(moveRefusal('auth/login.gctf', '../login.gctf', paths))
      .toBe('A path inside the collections folder — `..` leaves it.');
    expect(moveRefusal('auth/login.gctf', '/tmp/login.gctf', paths))
      .toBe('A path inside the collections folder — it cannot start with `/`.');
  });

  it('refuses a folder with no name after it', () => {
    expect(moveRefusal('auth/login.gctf', 'auth/', paths))
      .toBe('auth/ names a folder — the file needs a name of its own.');
  });

  it('says nothing about an empty answer or the name it has', () => {
    expect(moveRefusal('auth/login.gctf', '', paths)).toBeNull();
    expect(moveRefusal('auth/login.gctf', '  ', paths)).toBeNull();
    expect(moveRefusal('auth/login.gctf', 'auth/login.gctf', paths)).toBeNull();
    expect(moveRefusal('auth/login.gctf', './auth/login.gctf', paths)).toBeNull();
  });
});

describe('what a new name is refused for', () => {
  it('takes a free name', () => {
    expect(createRefusal('auth/signin.gctf', paths, 'file')).toBeNull();
    expect(createRefusal('smoke', paths, 'folder')).toBeNull();
  });

  it('refuses a folder that is already there', () => {
    expect(createRefusal('auth', paths, 'folder')).toBe('auth is already here — pick another name.');
  });

  it('refuses a folder named after a file, and a file named after a folder', () => {
    expect(createRefusal('auth/login.gctf', paths, 'folder'))
      .toBe('auth/login.gctf is already a file — pick another name.');
    expect(createRefusal('auth', paths, 'file')).toBe('auth is a folder — pick another name.');
  });

  it('refuses a file whose name is taken', () => {
    expect(createRefusal('auth/login.gctf', paths, 'file'))
      .toBe('auth/login.gctf already exists — pick another name.');
  });

  it('refuses a name that leaves the collection', () => {
    expect(createRefusal('../outside', paths, 'folder'))
      .toBe('A path inside the collections folder — `..` leaves it.');
  });

  it('takes a trailing slash on a folder and refuses it on a file', () => {
    expect(createRefusal('smoke/', paths, 'folder')).toBeNull();
    expect(createRefusal('smoke/', paths, 'file'))
      .toBe('smoke/ names a folder — the file needs a name of its own.');
  });

  it('says nothing about an empty name', () => {
    expect(createRefusal('   ', paths, 'file')).toBeNull();
  });
});
