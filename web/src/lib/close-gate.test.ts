import { describe, expect, it } from 'vitest';
import { closeWithGate } from './close-gate';

const spy = (saved: boolean) => {
  const done: string[] = [];
  return {
    done,
    actions: {
      close: () => { done.push('close'); },
      save: async () => { done.push('save'); return saved; },
      nameIt: () => { done.push('name'); },
    },
  };
};

describe('closing a tab that has edits', () => {
  it('closes on discard, and does not save', async () => {
    const { done, actions } = spy(true);
    expect(await closeWithGate('discard', { hasPath: true }, actions)).toBe('closed');
    expect(done).toEqual(['close']);
  });

  it('keeps the tab when the question was dismissed', async () => {
    const { done, actions } = spy(true);
    expect(await closeWithGate(null, { hasPath: true }, actions)).toBe('kept');
    expect(done).toEqual([]);
  });

  it('saves, then closes', async () => {
    const { done, actions } = spy(true);
    expect(await closeWithGate('save', { hasPath: true }, actions)).toBe('closed');
    expect(done).toEqual(['save', 'close']);
  });

  it('leaves the tab open when the save did not go through', async () => {
    const { done, actions } = spy(false);
    expect(await closeWithGate('save', { hasPath: true }, actions)).toBe('save-refused');
    expect(done).toEqual(['save']);
  });

  it('asks for a name instead of saving into nowhere', async () => {
    const { done, actions } = spy(true);
    expect(await closeWithGate('save', { hasPath: false }, actions)).toBe('named');
    expect(done).toEqual(['name']);
  });
});
