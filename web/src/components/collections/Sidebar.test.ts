import { describe, it, expect } from 'vitest';
import { buildTree } from '../../lib/tree';
import type { CollectionItem } from '../../lib/types';

describe('buildTree', () => {
  it('builds a flat tree from a single file', () => {
    const items: CollectionItem[] = [
      { path: 'test.gctf', name: 'test', is_dir: false },
    ];
    const tree = buildTree(items);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe('test.gctf');
    expect(tree[0].isDir).toBe(false);
    expect(tree[0].path).toBe('test.gctf');
  });

  it('builds a tree with a directory and nested file', () => {
    const items: CollectionItem[] = [
      { path: 'projects/myapi.gctf', name: 'myapi', is_dir: false },
    ];
    const tree = buildTree(items);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe('projects');
    expect(tree[0].isDir).toBe(true);
    expect(tree[0].path).toBe('projects');
    expect(tree[0].children).toHaveLength(1);
    expect(tree[0].children[0].name).toBe('myapi.gctf');
    expect(tree[0].children[0].isDir).toBe(false);
    expect(tree[0].children[0].path).toBe('projects/myapi.gctf');
  });

  it('treats empty directories (is_dir: true) as folders', () => {
    const items: CollectionItem[] = [
      { path: 'emptydir', name: 'emptydir', is_dir: true },
    ];
    const tree = buildTree(items);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe('emptydir');
    expect(tree[0].isDir).toBe(true);
    expect(tree[0].path).toBe('emptydir');
    expect(tree[0].children).toHaveLength(0);
  });

  it('marks leaf dir node as isDir when backend says is_dir: true', () => {
    const items: CollectionItem[] = [
      { path: 'projects', name: 'projects', is_dir: true },
    ];
    const tree = buildTree(items);
    expect(tree[0].isDir).toBe(true);
  });

  it('handles mixed files and empty directories at root', () => {
    const items: CollectionItem[] = [
      { path: 'test.gctf', name: 'test', is_dir: false },
      { path: 'emptydir', name: 'emptydir', is_dir: true },
    ];
    const tree = buildTree(items);
    expect(tree).toHaveLength(2);
    const dir = tree.find(n => n.name === 'emptydir');
    expect(dir).toBeDefined();
    expect(dir!.isDir).toBe(true);
    const file = tree.find(n => n.name === 'test.gctf');
    expect(file).toBeDefined();
    expect(file!.isDir).toBe(false);
  });

  it('directory with .gitkeep (no .gctf) is treated as directory', () => {
    const items: CollectionItem[] = [
      { path: 'projects', name: 'projects', is_dir: true },
    ];
    const tree = buildTree(items);
    expect(tree[0].isDir).toBe(true);
    expect(tree[0].children).toHaveLength(0);
  });

  it('deduplicates when multiple items share a parent path', () => {
    const items: CollectionItem[] = [
      { path: 'a/b.gctf', name: 'b', is_dir: false },
      { path: 'a/c.gctf', name: 'c', is_dir: false },
    ];
    const tree = buildTree(items);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe('a');
    expect(tree[0].children).toHaveLength(2);
  });

  it('returns empty array for empty input', () => {
    expect(buildTree([])).toHaveLength(0);
  });
});
