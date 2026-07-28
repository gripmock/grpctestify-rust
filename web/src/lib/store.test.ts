import { describe, it, expect } from 'vitest';
import { useStore, isRequestDirty, isTabDirty } from './store';
import type { CollectionParsed, Tab } from './types';

function makeParsed(overrides: Partial<CollectionParsed> = {}): CollectionParsed {
  return {
    endpoint: 'pkg.Svc/Method', address: 'localhost:4770', headers: {}, bodies: ['{}'],
    asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null,
    tls: {}, options: {}, bench: {}, proto: {},
    ...overrides,
  };
}

function makeTab(overrides: Partial<Tab> = {}): Tab {
  const orig = makeParsed();
  return {
    id: 't1', label: 'hello.gctf', endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies,
    environment: {}, response: null, requestTab: 'body', gctfTab: 'request', responseTab: 'response',
    collectionPath: 'hello.gctf', collectionParsed: orig, collectionOriginal: orig,
    rawContent: null, rawOriginal: null,
    ...overrides,
  };
}

function withState(request: { endpoint: string; headers: Record<string, string>; bodies: string[] }, workspaceOriginal: CollectionParsed | null) {
  return { ...useStore.getState(), request, workspaceOriginal };
}

describe('isRequestDirty', () => {
  it('false when there is no saved baseline (ad-hoc request, never saved)', () => {
    expect(isRequestDirty(withState({ endpoint: 'a', headers: {}, bodies: ['{}'] }, null))).toBe(false);
  });

  it('false when the live request matches the saved file', () => {
    const orig = makeParsed();
    expect(isRequestDirty(withState({ endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies }, orig))).toBe(false);
  });

  it('true when the body diverges from the saved file', () => {
    const orig = makeParsed();
    expect(isRequestDirty(withState({ endpoint: orig.endpoint, headers: orig.headers, bodies: ['{"edited":true}'] }, orig))).toBe(true);
  });

  it('true when the endpoint diverges from the saved file', () => {
    const orig = makeParsed();
    expect(isRequestDirty(withState({ endpoint: 'other.Svc/Method', headers: orig.headers, bodies: orig.bodies }, orig))).toBe(true);
  });

  it('true when headers diverge from the saved file', () => {
    const orig = makeParsed();
    expect(isRequestDirty(withState({ endpoint: orig.endpoint, headers: { 'x-extra': '1' }, bodies: orig.bodies }, orig))).toBe(true);
  });

  // Regression: comparing headers via JSON.stringify was sensitive to key insertion
  // order — HeadersEditor's rename path deletes then re-adds a key, reordering it
  // with no value change, which used to trip a false-positive dirty flag.
  it('false when headers have the same entries in a different order', () => {
    const orig = makeParsed({ headers: { a: '1', b: '2' } });
    expect(isRequestDirty(withState({ endpoint: orig.endpoint, headers: { b: '2', a: '1' }, bodies: orig.bodies }, orig))).toBe(false);
  });
});

describe('isTabDirty', () => {
  it('false for a tab that matches its saved snapshot', () => {
    expect(isTabDirty(makeTab())).toBe(false);
  });

  it('true when the tab body diverges from collectionOriginal', () => {
    expect(isTabDirty(makeTab({ bodies: ['{"edited":true}'] }))).toBe(true);
  });

  it('true when raw content diverges from rawOriginal, even if structured fields match', () => {
    expect(isTabDirty(makeTab({ rawContent: 'ADDRESS: x\n', rawOriginal: 'ADDRESS: y\n' }))).toBe(true);
  });

  it('false for an ad-hoc tab with no saved file (collectionOriginal null)', () => {
    expect(isTabDirty(makeTab({ collectionOriginal: null, endpoint: 'whatever' }))).toBe(false);
  });
});

// Regression: saveWorkspaceAs never set workspaceOriginal/collectionOriginal after
// saving a brand-new ad-hoc request, so the dirty baseline stayed null forever and
// isRequestDirty silently never flagged edits made after the first "Save As".
describe('saveWorkspaceAs', () => {
  it('sets workspaceOriginal so edits after Save As are correctly flagged dirty', async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: true, json: async () => ({}) })) as any;
    try {
      useStore.setState({
        request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] },
        address: 'localhost:4770',
        collectionParsed: null,
        workspaceOriginal: null,
      });
      await useStore.getState().saveWorkspaceAs('new-file');
      expect(useStore.getState().workspaceOriginal).not.toBeNull();
      expect(isRequestDirty(useStore.getState())).toBe(false);

      useStore.setState(s => ({ request: { ...s.request, bodies: ['{"edited":true}'] } }));
      expect(isRequestDirty(useStore.getState())).toBe(true);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});

// Regression: loadTab used to write a `collectionOriginal` key that doesn't exist on
// PlayStore (only Tab.collectionOriginal / PlayStore.workspaceOriginal do), so
// workspaceOriginal silently stayed null on every tab switch/collection load.
describe('addTab (via loadTab)', () => {
  it('restores workspaceOriginal from the tab collectionOriginal snapshot', () => {
    const orig = makeParsed({ bodies: ['{"name":"world"}'] });
    useStore.getState().addTab({
      collectionPath: 'hello.gctf',
      collectionParsed: orig,
      collectionOriginal: orig,
      endpoint: orig.endpoint,
      bodies: orig.bodies,
    });
    expect(useStore.getState().workspaceOriginal).toBe(orig);
  });
});
