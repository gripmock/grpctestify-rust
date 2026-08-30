import { describe, it, expect } from 'vitest';
import { runWithGate } from './run-gate';

function spy() {
  const calls: string[] = [];
  return {
    calls,
    actions: (saved: boolean) => ({
      save: async () => { calls.push('save'); return saved; },
      run: async () => { calls.push('run'); },
    }),
  };
}

describe('runWithGate', () => {
  it('runs the saved file when that is the choice', async () => {
    const s = spy();
    expect(await runWithGate('run', s.actions(true))).toBe('ran');
    expect(s.calls).toEqual(['run']);
  });

  it('saves first when asked, and then runs', async () => {
    const s = spy();
    expect(await runWithGate('save', s.actions(true))).toBe('saved-and-ran');
    expect(s.calls).toEqual(['save', 'run']);
  });

  it('does not run when the save was refused', async () => {
    const s = spy();
    expect(await runWithGate('save', s.actions(false))).toBe('save-refused');
    expect(s.calls).toEqual(['save']);
  });

  it('does nothing when the dialog was dismissed', async () => {
    const s = spy();
    expect(await runWithGate(null, s.actions(true))).toBe('cancelled');
    expect(s.calls).toEqual([]);
  });
});
