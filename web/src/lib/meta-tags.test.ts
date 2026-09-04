import { describe, expect, it } from 'vitest';
import { attributeMeta, attributeTags, tagsInUse } from './meta-tags';
import type { CollectionItem, SectionAttribute } from './types';

const attr = (name: string, value: string): SectionAttribute => ({ section: 'REQUEST', index: 0, name, value });
const file = (path: string, tags: string[]): CollectionItem => ({ path, name: path, is_dir: false, tags });

describe('the tags an attribute carries', () => {
  it('reads one attribute naming several', () => {
    expect(attributeTags([attr('tag', 'smoke, slow')])).toEqual(['smoke', 'slow']);
  });

  it('takes them from every section, once each', () => {
    expect(attributeTags([attr('tag', 'smoke'), attr('tag', 'smoke,api')])).toEqual(['smoke', 'api']);
  });

  it('is nothing when no attribute speaks for tags', () => {
    expect(attributeTags([attr('timeout', '5')])).toEqual([]);
    expect(attributeTags([attr('tag', ' , ')])).toEqual([]);
  });
});

describe('the tags this project already uses', () => {
  const suite = [
    file('a.gctf', ['smoke', 'api']),
    file('b.gctf', ['smoke']),
    file('c.gctf', ['slow']),
    { path: 'dir', name: 'dir', is_dir: true, tags: [] } as CollectionItem,
  ];

  it('counts the files carrying each, most used first', () => {
    expect(tagsInUse(suite, [], null)).toEqual([
      { tag: 'smoke', files: 2 },
      { tag: 'api', files: 1 },
      { tag: 'slow', files: 1 },
    ]);
  });

  it('does not offer what this file already carries', () => {
    expect(tagsInUse(suite, ['smoke'], null).map(t => t.tag)).toEqual(['api', 'slow']);
  });

  it('does not count the open file', () => {
    expect(tagsInUse(suite, [], 'b.gctf')).toEqual([
      { tag: 'api', files: 1 },
      { tag: 'slow', files: 1 },
      { tag: 'smoke', files: 1 },
    ]);
  });
});

describe('the owner and the summary an attribute carries', () => {
  const attrs = (...pairs: [string, string][]): SectionAttribute[] =>
    pairs.map(([name, value], index) => ({ section: 'REQUEST', index, name, value }));

  it('takes the first section that names one', () => {
    expect(attributeMeta(attrs(['owner', 'payments'], ['owner', 'billing']), 'owner')).toBe('payments');
    expect(attributeMeta(attrs(['summary', ' pays twice ']), 'summary')).toBe('pays twice');
  });

  it('is null when no section names one, and does not read the other name', () => {
    expect(attributeMeta(attrs(['tag', 'smoke']), 'owner')).toBeNull();
    expect(attributeMeta(attrs(['owner', 'payments']), 'summary')).toBeNull();
    expect(attributeMeta(attrs(['owner', '   ']), 'owner')).toBeNull();
  });
});
