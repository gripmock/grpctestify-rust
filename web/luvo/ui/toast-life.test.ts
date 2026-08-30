import { describe, it, expect } from 'vitest';
import { REFUSAL_TYPE, keepLast, repeatsNewest, toastLife } from 'luvo/ui/toast-life';

describe('toastLife', () => {
  /* The one kind worth reading twice used to leave while it was being read. */
  it('keeps a refusal until it is dismissed', () => {
    expect(toastLife('error')).toBeNull();
  });

  it('lets a confirmation go by itself', () => {
    expect(toastLife('success')).toBe(4000);
    expect(toastLife('info')).toBe(4000);
  });
});

describe('keepLast', () => {
  it('keeps the newest when a burst arrives', () => {
    expect(keepLast([1, 2, 3, 4, 5, 6])).toEqual([3, 4, 5, 6]);
  });

  it('leaves a short stack alone', () => {
    const items = [1, 2];
    expect(keepLast(items)).toBe(items);
  });
});

describe('repeatsNewest', () => {
  /* A refusal stays until it is dismissed, so a condition that keeps failing
     stacked the same sentence under itself until the cap. */
  it('is true for the same words as the newest', () => {
    const items = [
      { type: 'error' as const, message: 'gone' },
      { type: 'error' as const, message: 'still gone' },
    ];
    expect(repeatsNewest(items, { type: 'error', message: 'still gone' })).toBe(true);
  });

  it('is false for different words, a different kind, or an empty stack', () => {
    const items = [{ type: 'error' as const, message: 'gone' }];
    expect(repeatsNewest(items, { type: 'error', message: 'back' })).toBe(false);
    expect(repeatsNewest(items, { type: 'success', message: 'gone' })).toBe(false);
    expect(repeatsNewest([], { type: 'error', message: 'gone' })).toBe(false);
  });

  /* Only the newest: the same words said again after something else is news. */
  it('does not look past the newest', () => {
    const items = [
      { type: 'error' as const, message: 'gone' },
      { type: 'success' as const, message: 'back' },
    ];
    expect(repeatsNewest(items, { type: 'error', message: 'gone' })).toBe(false);
  });
});

describe('a warning', () => {
  it('stays until it is closed, the way a refusal does', () => {
    expect(toastLife('warn')).toBeNull();
    expect(toastLife('error')).toBeNull();
  });

  it('is not what a confirmation does', () => {
    expect(toastLife('success')).toBe(4000);
    expect(toastLife('info')).toBe(4000);
  });
});

/* A refusal is a fact about the state, not a failure: nothing was attempted,
   and the same click says it again. */
describe('a refusal', () => {
  it('goes on its own, the way a confirmation does', () => {
    expect(toastLife(REFUSAL_TYPE)).toBe(4000);
  });

  it('is not the kind that stays', () => {
    expect(toastLife(REFUSAL_TYPE)).not.toBeNull();
    expect(REFUSAL_TYPE).not.toBe('error');
    expect(REFUSAL_TYPE).not.toBe('warn');
  });
});
