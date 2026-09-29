import { describe, expect, it } from 'vitest';
import { cursorAt } from './use-cursor';

describe('where the highlight is', () => {
  it('starts at the first row when nothing has been picked', () => {
    expect(cursorAt(null, 'a', 5)).toBe(0);
  });

  it('goes back to the first row when the list is a different one', () => {
    expect(cursorAt({ key: 'a', at: 3 }, 'b', 5)).toBe(0);
  });

  it('never points past the end of a list that has narrowed', () => {
    expect(cursorAt({ key: 'a', at: 7 }, 'a', 3)).toBe(2);
    expect(cursorAt({ key: 'a', at: 7 }, 'a', 0)).toBe(0);
  });

  it('keeps the row that was picked while the list holds it', () => {
    expect(cursorAt({ key: 'a', at: 2 }, 'a', 5)).toBe(2);
  });
});
