import { describe, expect, it } from 'vitest';
import { stepIndex, treeStep, rowIsTabStop } from './tree-keys';

describe('treeStep', () => {
  it('maps the keys a tree is walked with', () => {
    expect(treeStep('ArrowDown')).toBe('next');
    expect(treeStep('ArrowUp')).toBe('prev');
    expect(treeStep('Home')).toBe('first');
    expect(treeStep('End')).toBe('last');
  });

  it('leaves every other key alone, including the ones that type', () => {
    for (const key of ['a', 'Enter', 'Tab', 'ArrowRight', 'ArrowLeft']) {
      expect(treeStep(key)).toBeNull();
    }
  });
});

describe('stepIndex', () => {
  it('stops at the ends instead of wrapping', () => {
    expect(stepIndex(0, 3, 'prev')).toBe(0);
    expect(stepIndex(2, 3, 'next')).toBe(2);
  });

  it('jumps to either end', () => {
    expect(stepIndex(1, 3, 'first')).toBe(0);
    expect(stepIndex(1, 3, 'last')).toBe(2);
  });

  it('has nowhere to go in an empty tree', () => {
    expect(stepIndex(0, 0, 'next')).toBe(-1);
  });
});

describe('the way into the tree', () => {
  it('is the row the workbench is on', () => {
    expect(rowIsTabStop('a.gctf', 'a.gctf', 'x.gctf')).toBe(true);
    expect(rowIsTabStop('b.gctf', 'a.gctf', 'x.gctf')).toBe(false);
  });

  it('is the first row when it is on none', () => {
    expect(rowIsTabStop('x.gctf', null, 'x.gctf')).toBe(true);
    expect(rowIsTabStop('a.gctf', null, 'x.gctf')).toBe(false);
  });
});
