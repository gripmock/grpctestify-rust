import { describe, it, expect } from 'vitest';
import { previewSlot, tabAtStake, tabHoldingCall } from './preview-slot';
import type { Tab } from './types';

function tab(over: Partial<Tab> = {}): Tab {
  return {
    id: 'x', label: 'a.gctf', endpoint: 'pkg.Svc/M', headers: {}, bodies: ['{}'],
    environment: {}, response: null, requestTab: 'body', gctfTab: 'request', responseTab: 'response',
    collectionPath: 'a.gctf', collectionParsed: null, collectionOriginal: null,
    rawContent: null, rawOriginal: null, documents: [],
    ...over,
  } as Tab;
}

describe('which tab a click may replace', () => {
  it('takes the preview slot', () => {
    expect(previewSlot([tab({ id: '1' }), tab({ id: '2', isPreview: true })])).toBe(1);
  });

  it('leaves a preview with unsaved edits alone', () => {
    const dirty = tab({ id: '2', isPreview: true, rawContent: 'edited', rawOriginal: 'original' });
    expect(previewSlot([tab({ id: '1' }), dirty])).toBe(-1);
  });

  it('has no slot when nothing is a preview', () => {
    expect(previewSlot([tab({ id: '1' })])).toBe(-1);
  });
});

describe('the tab already holding a call', () => {
  const call = { endpoint: 'pkg.Svc/M', headers: { a: '1' }, bodies: ['{"x":1}'] };
  const scratch = (over: Partial<Tab> = {}) =>
    tab({ collectionPath: null, endpoint: call.endpoint, headers: { ...call.headers }, bodies: [...call.bodies], ...over });

  it('is the scratch tab with the same request', () => {
    expect(tabHoldingCall([tab({ id: 'file' }), scratch({ id: 'held' })], call)?.id).toBe('held');
  });

  it('is not a tab whose body or headers differ', () => {
    expect(tabHoldingCall([scratch({ id: 'a', bodies: ['{"x":2}'] })], call)).toBeUndefined();
    expect(tabHoldingCall([scratch({ id: 'b', headers: {} })], call)).toBeUndefined();
  });

  it('is never a tab bound to a file', () => {
    expect(tabHoldingCall([tab({ id: 'f', collectionPath: 'a.gctf', headers: { a: '1' }, bodies: ['{"x":1}'] })], call))
      .toBeUndefined();
  });
});

describe('what closing a tab loses', () => {
  it('counts an edited file tab', () => {
    expect(tabAtStake(tab({ collectionPath: 'a.gctf' }), true)).toBe(true);
  });

  it('does not count a file tab that matches its file', () => {
    expect(tabAtStake(tab({ collectionPath: 'a.gctf' }), false)).toBe(false);
  });

  it('counts a request nobody has saved', () => {
    expect(tabAtStake(tab({ collectionPath: null, endpoint: 'pkg.Svc/M' }), false)).toBe(true);
  });

  it('does not count a tab nobody has typed into', () => {
    const fresh = tab({ collectionPath: null, endpoint: '', headers: {}, bodies: ['{}'], rawContent: null });
    expect(tabAtStake(fresh, false)).toBe(false);
  });
});
