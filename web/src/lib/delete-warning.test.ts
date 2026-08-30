import { describe, it, expect } from 'vitest';
import { deleteQuestion, deleteScope, referencedNote, renameBreaksNote, unsavedNote } from './delete-warning';
import type { TreeNode } from './types';

const file = (path: string): TreeNode => ({ name: path.split('/').pop()!, path, isDir: false, children: [] });
const dir = (path: string, children: TreeNode[]): TreeNode =>
  ({ name: path.split('/').pop()!, path, isDir: true, children });

describe('what a delete takes with it', () => {
  const tree = dir('auth', [file('auth/login.gctf'), dir('auth/mfa', [file('auth/mfa/enroll.gctf')])]);

  it('counts every file under a folder', () => {
    const scope = deleteScope(tree, []);
    expect(scope.files).toBe(2);
    expect(deleteQuestion(tree, scope)).toContain('2 files inside go with it');
  });

  it('says nothing extra about a single file', () => {
    const scope = deleteScope(file('a.gctf'), []);
    expect(deleteQuestion(file('a.gctf'), scope)).toBe('Delete "a.gctf"? The file is removed from disk.');
  });

  it('names the unsaved work that would go with it', () => {
    const scope = deleteScope(tree, [
      { path: 'auth/login.gctf', dirty: true },
      { path: 'auth/mfa/enroll.gctf', dirty: false },
      { path: 'other.gctf', dirty: true },
    ]);
    expect(scope.unsaved).toEqual(['auth/login.gctf']);
    expect(unsavedNote(scope)).toContain('unsaved edits open');
  });

  it('counts unsaved tabs of the file itself', () => {
    const scope = deleteScope(file('a.gctf'), [{ path: 'a.gctf', dirty: true }]);
    expect(unsavedNote(scope)).toBe('a.gctf has unsaved edits open — they go too.');
  });

  it('has nothing to add when everything is saved', () => {
    expect(unsavedNote(deleteScope(tree, [{ path: 'auth/login.gctf', dirty: false }]))).toBe(null);
  });

  it('does not count a folder whose name merely starts the same', () => {
    const scope = deleteScope(tree, [{ path: 'authority/x.gctf', dirty: true }]);
    expect(scope.unsaved).toEqual([]);
  });
});

describe('what names the file being deleted', () => {
  it('names the files, and counts the rest', () => {
    expect(referencedNote(['a.gctf'])).toBe('a.gctf names it — those files lose their schema.');
    expect(referencedNote(['a.gctf', 'b.gctf', 'c.gctf', 'd.gctf']))
      .toContain('a.gctf, b.gctf, c.gctf and 1 more name it');
  });

  it('says nothing when nothing names it', () => {
    expect(referencedNote([])).toBe(null);
  });
});

describe('what a rename leaves behind', () => {
  it('names the files that will not follow', () => {
    expect(renameBreaksNote(['auth/login.gctf']))
      .toBe('auth/login.gctf names it by the old path and will not follow — those files lose their schema.');
  });

  it('counts the rest past three', () => {
    expect(renameBreaksNote(['a.gctf', 'b.gctf', 'c.gctf', 'd.gctf', 'e.gctf']))
      .toBe('a.gctf, b.gctf, c.gctf and 2 more name it by the old path and will not follow — those files lose their schema.');
  });

  it('says nothing about a file nothing names', () => {
    expect(renameBreaksNote([])).toBeNull();
  });
});
