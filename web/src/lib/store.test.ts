import { describe, it, expect, afterEach, vi } from 'vitest';
import { bindingsOf, copyNote, rawAuthorityReason, rawAuthorityRefusal, useStore, callAddress, formsAheadOfFile, keepFromAnotherRoot, tabFileMissing, projectCallEnv, resolveProjectAddress, contentUnread, isRequestDirty, isTabDirty, isActiveTabDirty, rawIsAuthoritative, structuredSave, serializeTab, deserializeTab, MAX_STORED_RAW, fileMissing } from './store';
import type { CollectionParsed, Tab } from './types';

function makeParsed(overrides: Partial<CollectionParsed> = {}): CollectionParsed {
  return {
    endpoint: 'pkg.Svc/Method', address: 'localhost:4770', headers: {}, bodies: ['{}'],
    asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
    tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
    expect_responses: [], expect_error: null,
    ...overrides,
  };
}

function makeTab(overrides: Partial<Tab> = {}): Tab {
  const orig = makeParsed();
  return {
    id: 't1', label: 'hello.gctf', endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies,
    response: null, requestTab: 'body', gctfTab: 'request', responseTab: 'response',
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

describe('saveWorkspace and the text of the file', () => {
  it('brings the source tab up to what was just written', async () => {
    const before = '--- ENDPOINT ---\npkg.Svc/Method\n';
    const after = '--- ENDPOINT ---\npkg.Svc/Method\n\n--- OPTIONS ---\nprotocol: grpc-web\n';
    const orig = makeParsed();
    const tab = makeTab({ collectionParsed: orig, collectionOriginal: orig, rawContent: before, rawOriginal: before });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id,
      workspacePath: 'hello.gctf', collectionParsed: orig, workspaceOriginal: orig,
      request: { endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies },
      rawContent: before, rawOriginal: before,
      protocol: 'grpc-web', protocolTouched: true,
    });
    const originalFetch = globalThis.fetch;
    globalThis.fetch = (async (url: string) => ({
      ok: true,
      json: async () => (String(url).includes('/api/collections/')
        ? { content: after, parsed: orig, version: { mtime_ms: 2, hash: 'b' } }
        : { mtime_ms: 2, hash: 'b' }),
    })) as any;
    try {
      expect(await useStore.getState().saveWorkspace()).toBe(true);
    } finally {
      globalThis.fetch = originalFetch;
    }
    const st = useStore.getState();
    expect(st.rawContent).toBe(after);
    expect(st.rawOriginal).toBe(after);
    expect(st.tabs[0].rawContent).toBe(after);
  });
});

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

describe('draft asserts and extracts', () => {
  function openTab() {
    const orig = makeParsed({ asserts: ['.ok == true'], extracts: { token: '.auth.token' } });
    const tab = makeTab({ collectionParsed: orig, collectionOriginal: orig });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id,
      collectionParsed: orig, workspaceOriginal: orig,
      request: { endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies },
    });
    return orig;
  }

  it('addAssert appends, dedupes, and marks the request dirty', () => {
    openTab();
    expect(useStore.getState().addAssert('.id != ""')).toBe('added');
    expect(useStore.getState().addAssert('.id != ""')).toBe('duplicate');
    expect(useStore.getState().addAssert('   ')).toBe('empty');
    const st = useStore.getState();
    expect(st.collectionParsed!.asserts).toEqual(['.ok == true', '.id != ""']);
    expect(isRequestDirty(st)).toBe(true);
    expect(st.tabs[0].collectionParsed!.asserts).toContain('.id != ""');
    expect(isTabDirty(st.tabs[0])).toBe(true);
  });

  it('removeAssert brings the draft back to clean', () => {
    openTab();
    useStore.getState().addAssert('.id != ""');
    useStore.getState().removeAssert(1);
    expect(isRequestDirty(useStore.getState())).toBe(false);
  });

  it('addExtract and removeExtract round-trip', () => {
    openTab();
    useStore.getState().addExtract('user_id', '.user.id');
    expect(useStore.getState().collectionParsed!.extracts).toEqual({ token: '.auth.token', user_id: '.user.id' });
    expect(isRequestDirty(useStore.getState())).toBe(true);
    useStore.getState().removeExtract('user_id');
    expect(isRequestDirty(useStore.getState())).toBe(false);
  });

  it('addAssert on an untitled tab creates the draft shell', () => {
    const tab = makeTab({ collectionPath: null, collectionParsed: null, collectionOriginal: null });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id,
      collectionParsed: null, workspaceOriginal: null,
      request: { endpoint: 'pkg.Svc/M', headers: {}, bodies: ['{}'] },
    });
    useStore.getState().addAssert('.ok == true');
    expect(useStore.getState().collectionParsed!.asserts).toEqual(['.ok == true']);
  });
});

describe('saving OPTIONS', () => {
  function open(
    options: Record<string, string>,
    protocol: 'grpc' | 'grpc-web' | 'connectrpc',
    protocolTouched = true,
  ) {
    const orig = makeParsed({ options });
    const tab = makeTab({ collectionParsed: orig, collectionOriginal: orig });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id,
      collectionParsed: orig, workspaceOriginal: orig, workspacePath: 'hello.gctf',
      request: { endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies },
      protocol, protocolTouched,
    });
  }

  async function captureSave(): Promise<any> {
    let body: any = null;
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).includes('/api/save')) body = JSON.parse(String(init?.body));
      return { ok: true, json: async () => ({ mtime_ms: 1, hash: 'sha256:x' }) } as Response;
    }) as typeof fetch;
    try {
      expect(await useStore.getState().saveWorkspace()).toBe(true);
    } finally {
      globalThis.fetch = original;
    }
    return body;
  }

  it('keeps the file\'s other OPTIONS keys when the transport is written', async () => {
    open({ timeout: '10', retry: '3' }, 'grpc-web');
    const body = await captureSave();
    expect(body.options).toEqual([['timeout', '10'], ['retry', '3'], ['protocol', 'grpc-web']]);
  });

  it('removes protocol when the transport is set back to the default for this file', async () => {
    open({ timeout: '10', protocol: 'grpc-web' }, 'grpc');
    const body = await captureSave();
    expect(body.options).toEqual([['timeout', '10']]);
  });

  it('keeps the file\'s transport when nobody chose another for it', async () => {
    open({ timeout: '10', protocol: 'grpc-web' }, 'grpc', false);
    const body = await captureSave();
    expect(body.options).toEqual([['timeout', '10'], ['protocol', 'grpc-web']]);
  });

  it('carries an empty OPTIONS when the file has none and the transport is the default', async () => {
    open({}, 'grpc');
    const body = await captureSave();
    expect(body.options).toEqual([]);
  });
});

describe('the preview slot', () => {
  function historyEntry(id: string, endpoint: string) {
    return {
      id, timestamp: 1, endpoint, bodies: ['{}'], headers: {},
      response: { status: 'ok' as const, statusCode: 0, messages: [{}], headers: {}, trailers: {}, error: null, durationMs: 1 },
    };
  }

  it('reuses a clean preview tab', () => {
    useStore.setState({ tabs: [], activeTabId: null, history: [] });
    useStore.getState().restoreHistory(historyEntry('h1', 'pkg.Svc/A'));
    useStore.getState().restoreHistory(historyEntry('h2', 'pkg.Svc/B'));
    const st = useStore.getState();
    expect(st.tabs).toHaveLength(1);
    expect(st.tabs[0].endpoint).toBe('pkg.Svc/B');
  });

  it('never reuses a preview tab that has unsaved edits', () => {
    useStore.setState({ tabs: [], activeTabId: null, history: [] });
    useStore.getState().restoreHistory(historyEntry('h1', 'pkg.Svc/A'));

    const orig = makeParsed({ endpoint: 'pkg.Svc/A' });
    useStore.setState(s => ({
      tabs: s.tabs.map(t => ({ ...t, collectionOriginal: orig, collectionParsed: orig, bodies: ['{"edited":true}'] })),
    }));

    useStore.getState().restoreHistory(historyEntry('h2', 'pkg.Svc/B'));
    const st = useStore.getState();
    expect(st.tabs).toHaveLength(2);
    expect(st.tabs[0].bodies).toEqual(['{"edited":true}']);
  });
});

describe('a section the forms can edit counts as an edit', () => {
  const cases: [string, Partial<CollectionParsed>][] = [
    ['OPTIONS', { options: { timeout: '5' } }],
    ['TLS', { tls: { ca: '/tmp/ca.pem' } }],
    ['META', { meta_name: 'login flow' }],
    ['tags', { meta_tags: ['smoke'] }],
    ['PROTO', { proto: { files: 'auth.proto' } }],
    ['BENCH', { bench: { concurrency: '50' } }],
    ['DATASET', { dataset: [{ id: '1' }] }],
    ['asserts', { asserts: ['.ok == true'] }],
    ['extracts', { extracts: { token: '.token' } }],
  ];

  for (const [name, edit] of cases) {
    it(`marks a tab dirty when ${name} diverges`, () => {
      const orig = makeParsed();
      expect(isTabDirty(makeTab({ collectionOriginal: orig, collectionParsed: makeParsed(edit) }))).toBe(true);
    });
  }

  it('leaves a tab clean when every section still matches', () => {
    const orig = makeParsed();
    expect(isTabDirty(makeTab({ collectionOriginal: orig, collectionParsed: makeParsed() }))).toBe(false);
  });
});

describe('restoring a call from history', () => {
  const entry = {
    id: 'restore-1', timestamp: 1, endpoint: 'pkg.Svc/Method', bodies: ['{}'], headers: {},
    response: { status: 'ok' as const, statusCode: 0, messages: [{}], headers: {}, trailers: {}, error: null, durationMs: 1 },
    connection: { address: 'staging:8443', protocol: 'grpc-web' as const, tls: true, tlsInsecure: false },
  };

  it('goes back to the target the call was made against', () => {
    useStore.setState({ tabs: [], activeTabId: null, address: 'localhost:4770', protocol: 'grpc', tls: false, tlsInsecure: true });
    useStore.getState().restoreHistory(entry);
    const st = useStore.getState();
    expect(st.address).toBe('staging:8443');
    expect(st.protocol).toBe('grpc-web');
    expect(st.tls).toBe(true);
    expect(st.tlsInsecure).toBe(false);
    expect(st.request.endpoint).toBe('pkg.Svc/Method');
  });

  it('names an HTTP call after the thing it addresses', () => {
    useStore.setState({ tabs: [], activeTabId: null });
    useStore.getState().restoreHistory({ ...entry, id: 'restore-http', endpoint: 'GET /v1/users?page=2' });
    expect(useStore.getState().tabs.at(-1)!.label).toBe('users');
  });

  it('still names a gRPC call after its method', () => {
    useStore.setState({ tabs: [], activeTabId: null });
    useStore.getState().restoreHistory({ ...entry, id: 'restore-grpc' });
    expect(useStore.getState().tabs.at(-1)!.label).toBe('Method');
  });

  it('keeps the transport when the call recorded none', () => {
    useStore.setState({ tabs: [], activeTabId: null, protocol: 'grpc-web' });
    useStore.getState().restoreHistory({
      ...entry,
      id: 'restore-http-conn',
      endpoint: 'GET /v1/users',
      connection: { address: 'http://api:8899', tls: false } as never,
    });
    const st = useStore.getState();
    expect(st.address).toBe('http://api:8899');
    expect(st.protocol).toBe('grpc-web');
  });

  it('leaves the connection alone for an entry recorded before it was kept', () => {
    useStore.setState({ tabs: [], activeTabId: null, address: 'localhost:4770', protocol: 'grpc', tls: false });
    const { connection: _drop, ...older } = { ...entry, id: 'restore-2' };
    useStore.getState().restoreHistory(older);
    const st = useStore.getState();
    expect(st.address).toBe('localhost:4770');
    expect(st.protocol).toBe('grpc');
    expect(st.request.endpoint).toBe('pkg.Svc/Method');
  });
});

describe('a save the file underneath has moved on from', () => {
  const parsed = makeParsed();
  const tab = makeTab({ collectionPath: 'one.gctf' });

  function armTab() {
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'one.gctf',
      request: { endpoint: parsed.endpoint, headers: parsed.headers, bodies: parsed.bodies },
      collectionParsed: parsed, workspaceOriginal: parsed, saveConflict: null,
      rawContent: null, rawOriginal: null, activeStep: 0,
    });
  }

  function stubFetch(answers: [string, () => Response][]) {
    const seen: string[] = [];
    globalThis.fetch = (async (url: string) => {
      seen.push(String(url));
      const hit = answers.find(([prefix]) => String(url).startsWith(prefix));
      if (!hit) return new Response('{}', { status: 200 });
      return hit[1]();
    }) as typeof fetch;
    return seen;
  }

  const conflict = () => new Response(
    JSON.stringify({
      error: 'stale version',
      version: { mtime_ms: 2, hash: 'sha256:theirs' },
      content: '--- ENDPOINT ---\ntheirs.Svc/Method\n',
    }),
    { status: 409, headers: { 'Content-Type': 'application/json' } },
  );

  it('stops and holds both versions instead of overwriting', async () => {
    armTab();
    stubFetch([['/api/save-structured', conflict]]);
    expect(await useStore.getState().saveWorkspace()).toBe(false);

    const held = useStore.getState().saveConflict;
    expect(held?.path).toBe('one.gctf');
    expect(held?.theirs).toContain('theirs.Svc/Method');
    expect(held?.mine).toContain(parsed.endpoint);
  });

  it('retries unconditionally when the answer is "mine wins"', async () => {
    armTab();
    stubFetch([['/api/save-structured', conflict]]);
    await useStore.getState().saveWorkspace();

    let bodies: string[] = [];
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).startsWith('/api/save-structured')) {
        bodies.push(String(init?.body ?? ''));
        return new Response(JSON.stringify({ mtime_ms: 3, hash: 'sha256:mine' }), { status: 200 });
      }
      return new Response('[]', { status: 200 });
    }) as typeof fetch;

    await useStore.getState().resolveSaveConflict('overwrite');
    expect(useStore.getState().saveConflict).toBeNull();
    expect(bodies).toHaveLength(1);
    expect(JSON.parse(bodies[0]).version).toBeUndefined();
  });

  it('takes the file on disk when the answer is "theirs wins"', async () => {
    armTab();
    stubFetch([['/api/save-structured', conflict]]);
    await useStore.getState().saveWorkspace();

    const theirParsed = { ...makeParsed(), endpoint: 'theirs.Svc/Method' };
    stubFetch([['/api/collections/', () => new Response(
      JSON.stringify({
        content: '--- ENDPOINT ---\ntheirs.Svc/Method\n',
        parsed: theirParsed,
        documents: [],
        version: { mtime_ms: 2, hash: 'sha256:theirs' },
      }),
      { status: 200 },
    )]]);

    await useStore.getState().resolveSaveConflict('reload');
    const st = useStore.getState();
    expect(st.saveConflict).toBeNull();
    expect(st.rawContent).toContain('theirs.Svc/Method');
    expect(st.rawOriginal).toBe(st.rawContent);
    expect(st.request.endpoint).toBe('theirs.Svc/Method');
  });

  it('leaves everything alone when the answer is "neither yet"', async () => {
    armTab();
    stubFetch([['/api/save-structured', conflict]]);
    await useStore.getState().saveWorkspace();
    await useStore.getState().resolveSaveConflict('cancel');

    const st = useStore.getState();
    expect(st.saveConflict).toBeNull();
    expect(st.request.endpoint).toBe(parsed.endpoint);
    expect(st.rawContent).toBeNull();
  });
});

describe('two editors over one file', () => {
  const parsed = makeParsed();
  const tab = makeTab({ collectionPath: 'one.gctf' });

  function armRaw(rawContent: string | null, rawOriginal: string | null) {
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'one.gctf',
      request: { endpoint: parsed.endpoint, headers: parsed.headers, bodies: parsed.bodies },
      collectionParsed: parsed, workspaceOriginal: parsed, saveConflict: null,
      rawContent, rawOriginal, activeStep: 0,
    });
  }

  it('knows which side a save must write', () => {
    expect(rawIsAuthoritative({ rawContent: null, rawOriginal: null })).toBe(false);
    expect(rawIsAuthoritative({ rawContent: 'same', rawOriginal: 'same' })).toBe(false);
    expect(rawIsAuthoritative({ rawContent: 'edited', rawOriginal: 'loaded' })).toBe(true);
    expect(rawIsAuthoritative({ rawContent: 'scaffolded', rawOriginal: null })).toBe(true);
  });

  it('saves the source when the source is the edited side', async () => {
    armRaw('--- ENDPOINT ---\nedited.Svc/M\n', '--- ENDPOINT ---\nloaded.Svc/M\n');
    const hit: string[] = [];
    globalThis.fetch = (async (url: string) => {
      hit.push(String(url));
      if (String(url).startsWith('/api/collections/')) {
        return new Response(JSON.stringify({ content: '', parsed: makeParsed(), documents: [], version: { mtime_ms: 1, hash: 'h' } }), { status: 200 });
      }
      return new Response(JSON.stringify({ mtime_ms: 1, hash: 'h' }), { status: 200 });
    }) as typeof fetch;

    await useStore.getState().saveWorkspace();
    expect(hit[0]).toBe('/api/save');
    expect(hit).not.toContain('/api/save-structured');
  });

  it('saves the forms when the source has not been touched', async () => {
    armRaw('--- ENDPOINT ---\nloaded.Svc/M\n', '--- ENDPOINT ---\nloaded.Svc/M\n');
    const hit: string[] = [];
    globalThis.fetch = (async (url: string) => {
      hit.push(String(url));
      return new Response(JSON.stringify({ mtime_ms: 1, hash: 'h' }), { status: 200 });
    }) as typeof fetch;

    await useStore.getState().saveWorkspace();
    expect(hit[0]).toBe('/api/save-structured');
  });
});

describe('a verdict belongs to the version that was run', () => {
  const parsed = makeParsed();
  const tab = makeTab({ collectionPath: 'auth/login.gctf' });

  it('is dropped when the file is saved', async () => {
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'auth/login.gctf',
      request: { endpoint: parsed.endpoint, headers: parsed.headers, bodies: parsed.bodies },
      collectionParsed: parsed, workspaceOriginal: parsed, rawContent: null, rawOriginal: null,
      activeStep: 0,
      run: {
        ...useStore.getState().run,
        verdicts: {
          'auth/login.gctf': { path: 'auth/login.gctf', state: 'fail', message: 'assertion failed' },
          'feed/crud.gctf': { path: 'feed/crud.gctf', state: 'pass' },
        },
      },
    });

    globalThis.fetch = (async (url: string) => {
      if (String(url).startsWith('/api/collections/')) {
        return new Response(JSON.stringify({ content: '', parsed, documents: [], version: { mtime_ms: 1, hash: 'h' } }), { status: 200 });
      }
      return new Response(JSON.stringify({ mtime_ms: 1, hash: 'h' }), { status: 200 });
    }) as typeof fetch;

    await useStore.getState().saveWorkspace();

    const verdicts = useStore.getState().run.verdicts;
    expect(verdicts['auth/login.gctf']).toBeUndefined();
    expect(verdicts['feed/crud.gctf']?.state).toBe('pass');
  });
});

describe('a verdict follows the file it is about', () => {
  it('moves with a rename instead of marking a path that no longer exists', () => {
    useStore.setState({
      tabs: [], activeTabId: null, workspacePath: null, selectedCollection: null,
      run: {
        ...useStore.getState().run,
        verdicts: {
          'auth/login.gctf': { path: 'auth/login.gctf', state: 'fail' },
          'feed/crud.gctf': { path: 'feed/crud.gctf', state: 'pass' },
        },
      },
    });

    useStore.getState().retargetPath('auth/login.gctf', 'auth/signin.gctf');

    const verdicts = useStore.getState().run.verdicts;
    expect(verdicts['auth/login.gctf']).toBeUndefined();
    expect(verdicts['auth/signin.gctf']).toEqual({ path: 'auth/signin.gctf', state: 'fail' });
    expect(verdicts['feed/crud.gctf']?.state).toBe('pass');
  });
});

describe('isActiveTabDirty', () => {
  it('does not call a step change an edit', () => {
    const stepOne = makeParsed({ endpoint: 'a.A/One' });
    const stepTwo = makeParsed({ endpoint: 'a.A/Two', asserts: ['.ok == true'] });
    const tab = makeTab({ collectionParsed: stepOne, collectionOriginal: stepOne, endpoint: 'a.A/One' });
    expect(isActiveTabDirty(tab, {
      request: { endpoint: stepTwo.endpoint, headers: stepTwo.headers, bodies: stepTwo.bodies },
      rawContent: null,
      rawOriginal: null,
      workspaceOriginal: stepTwo,
      collectionParsed: stepTwo,
      address: '',
      addressTouched: false,
      protocol: 'grpc',
      protocolTouched: false,
    })).toBe(false);
  });

  it('still sees a real edit to the step on screen', () => {
    const step = makeParsed({ endpoint: 'a.A/Two' });
    const tab = makeTab({ collectionParsed: step, collectionOriginal: step });
    expect(isActiveTabDirty(tab, {
      request: { endpoint: 'a.A/Other', headers: step.headers, bodies: step.bodies },
      rawContent: null,
      rawOriginal: null,
      workspaceOriginal: step,
      collectionParsed: step,
      address: '',
      addressTouched: false,
      protocol: 'grpc',
      protocolTouched: false,
    })).toBe(true);
  });

  it('counts a transport chosen for this file as an edit', () => {
    const step = makeParsed({ endpoint: 'a.A/Two' });
    const tab = makeTab({ collectionParsed: step, collectionOriginal: step });
    const live = {
      request: { endpoint: step.endpoint, headers: step.headers, bodies: step.bodies },
      rawContent: null,
      rawOriginal: null,
      workspaceOriginal: step,
      collectionParsed: step,
      address: '',
      addressTouched: false,
    };
    expect(isActiveTabDirty(tab, { ...live, protocol: 'grpc-web', protocolTouched: true })).toBe(true);
    expect(isActiveTabDirty(tab, { ...live, protocol: 'grpc-web', protocolTouched: false })).toBe(false);
    expect(isActiveTabDirty(tab, { ...live, protocol: 'grpc', protocolTouched: true })).toBe(false);
  });

  it('does not call the file\'s own transport an edit', () => {
    const step = makeParsed({ endpoint: 'a.A/Two', options: { protocol: 'connectrpc' } });
    const tab = makeTab({ collectionParsed: step, collectionOriginal: step });
    expect(isActiveTabDirty(tab, {
      request: { endpoint: step.endpoint, headers: step.headers, bodies: step.bodies },
      rawContent: null,
      rawOriginal: null,
      workspaceOriginal: step,
      collectionParsed: step,
      address: '',
      addressTouched: false,
      protocol: 'connectrpc',
      protocolTouched: true,
    })).toBe(false);
  });
});

describe('the expected outcome', () => {
  it('replaces one with the other', () => {
    const st = useStore.getState();
    st.setExpectMode('response');
    expect(useStore.getState().collectionParsed?.expect_responses).toHaveLength(1);
    st.setExpectMode('error');
    expect(useStore.getState().collectionParsed?.expect_responses).toEqual([]);
    expect(useStore.getState().collectionParsed?.expect_error?.body).toBe('{}');
    st.setExpectMode('none');
    expect(useStore.getState().collectionParsed?.expect_error).toBeNull();
    expect(useStore.getState().collectionParsed?.expect_responses).toEqual([]);
  });

  it('carries the expectation into the save payload', () => {
    useStore.getState().setExpectMode('response');
    useStore.getState().setExpectResponse(0, { body: '{"ok": true}', partial: true });
    const body = structuredSave(useStore.getState());
    expect(body.expect).toEqual({
      responses: [{ body: '{"ok": true}', partial: true, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }],
      error: null,
    });
    useStore.getState().setExpectMode('none');
  });
});

describe('sharing', () => {
  it('is a link for a saved file — no dialog to open', () => {
    const tab = makeTab({ collectionPath: 'examples/basic/unary.gctf' });
    useStore.setState({ tabs: [tab], activeTabId: tab.id, share: null });
    expect(useStore.getState().startShare()).toBe('link');
    expect(useStore.getState().share).toBeNull();
  });

  it('opens the dialog for a request with no file, with credentials unticked', () => {
    const tab = makeTab({ collectionPath: null });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, share: null,
      request: { endpoint: 'pkg.Svc/M', headers: { authorization: 'Bearer x', 'x-tenant': 'acme' }, bodies: ['{}'] },
    });
    expect(useStore.getState().startShare()).toBe('dialog');
    expect(useStore.getState().share?.headers).toEqual({ authorization: false, 'x-tenant': true });
  });

  it('closes and forgets what was ticked', () => {
    const tab = makeTab({ collectionPath: null });
    useStore.setState({ tabs: [tab], activeTabId: tab.id, share: null, request: { endpoint: '', headers: {}, bodies: ['{}'] } });
    useStore.getState().startShare();
    useStore.getState().closeShare();
    expect(useStore.getState().share).toBeNull();
  });
});

describe('a second empty tab', () => {
  const emptyState = () => {
    useStore.setState({
      tabs: [], activeTabId: null, request: { endpoint: '', headers: {}, bodies: ['{}'] },
      rawContent: null, workspacePath: null, addressTouched: false, collectionParsed: null,
    });
  };

  it('is the empty tab that is already open', () => {
    emptyState();
    const first = useStore.getState().addTab();
    const second = useStore.getState().addTab();
    expect(second).toBe(first);
    expect(useStore.getState().tabs).toHaveLength(1);
  });

  it('is a new tab once something has been typed into the first', () => {
    emptyState();
    const first = useStore.getState().addTab();
    useStore.setState({ request: { endpoint: 'pkg.Svc/M', headers: {}, bodies: ['{}'] } });
    const second = useStore.getState().addTab();
    expect(second).not.toBe(first);
    expect(useStore.getState().tabs).toHaveLength(2);
  });

  it('is a new tab whenever the caller brings something to put in it', () => {
    emptyState();
    const first = useStore.getState().addTab();
    const second = useStore.getState().addTab({ endpoint: 'pkg.Svc/Imported' });
    expect(second).not.toBe(first);
    expect(useStore.getState().tabs).toHaveLength(2);
  });
});

describe('expectFromResponse', () => {
  it('writes the messages that came back as the RESPONSE expectation', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: '' }),
      response: {
        status: 'ok', messages: [{ message: 'Hello' }], headers: {}, trailers: {},
        error: null, statusCode: 0, durationMs: 3,
      } as never,
    });
    expect(useStore.getState().expectFromResponse()).toBe(true);
    const p = useStore.getState().collectionParsed!;
    expect(p.expect_responses).toHaveLength(1);
    expect(JSON.parse(p.expect_responses[0].body)).toEqual({ message: 'Hello' });
    expect(p.expect_error).toBeNull();
    expect(useStore.getState().requestTab).toBe('asserts');
  });

  it('writes a failure as the ERROR expectation, with its code', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: '' }),
      response: {
        status: 'error', messages: [], headers: {}, trailers: {},
        error: 'gRPC error code=5 message=No matching stub found', statusCode: 5, durationMs: 3,
      } as never,
    });
    expect(useStore.getState().expectFromResponse()).toBe(true);
    const p = useStore.getState().collectionParsed!;
    expect(p.expect_responses).toHaveLength(0);
    expect(JSON.parse(p.expect_error!.body)).toEqual({ code: 5, message: 'No matching stub found' });
  });

  it('has nothing to write before a call', () => {
    useStore.setState({ collectionParsed: makeParsed({ address: '' }), response: null });
    expect(useStore.getState().expectFromResponse()).toBe(false);
  });
});

describe('where a call goes', () => {
  it('is the file\'s ADDRESS whatever the header holds', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: 'localhost:50051' }),
      address: 'other:9000',
      protocol: 'grpc',
    });
    expect(callAddress(useStore.getState())).toBe('localhost:50051');
  });

  it('is the header when the file names none', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: '' }),
      address: 'other:9000',
      protocol: 'grpc',
    });
    expect(callAddress(useStore.getState())).toBe('other:9000');
  });
});

describe('the address a save writes', () => {
  it('leaves a file without an ADDRESS without one', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: '' }),
      address: 'somewhere-else:4770',
      addressTouched: false,
    });
    expect(structuredSave(useStore.getState()).address).toBeUndefined();
  });

  it('carries it once it was typed for this file', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: '' }),
      address: 'staging:4770',
      addressTouched: true,
    });
    expect(structuredSave(useStore.getState()).address).toBe('staging:4770');
  });

  it('keeps the file\'s own when nothing was typed for it', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: 'localhost:4770' }),
      address: '',
      addressTouched: false,
    });
    expect(structuredSave(useStore.getState()).address).toBe('localhost:4770');
  });

  it('keeps the file\'s own even while the field holds another tab\'s host', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: 'localhost:4770' }),
      address: 'somewhere-else:9000',
      addressTouched: false,
    });
    expect(structuredSave(useStore.getState()).address).toBe('localhost:4770');
  });

  it('takes the address off a file when the field is cleared for it', () => {
    useStore.setState({
      collectionParsed: makeParsed({ address: 'localhost:4770' }),
      address: '',
      addressTouched: true,
    });
    expect(structuredSave(useStore.getState()).address).toBeUndefined();
  });

  it('carries it for a file that does not exist yet', () => {
    useStore.setState({ collectionParsed: null, address: 'localhost:4770', addressTouched: false });
    expect(structuredSave(useStore.getState()).address).toBe('localhost:4770');
  });
});

describe('an address typed for the open file', () => {
  it('makes the tab dirty, so a save has something to write', () => {
    const orig = makeParsed({ address: 'localhost:4770' });
    useStore.setState({
      collectionParsed: orig,
      workspaceOriginal: orig,
      request: { endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies },
      rawContent: null,
      rawOriginal: null,
      address: 'staging:4770',
      addressTouched: true,
    });
    expect(isRequestDirty(useStore.getState())).toBe(true);
  });

  it('is not an edit while the field is only carrying the last host used', () => {
    const orig = makeParsed({ address: '' });
    useStore.setState({
      collectionParsed: orig,
      workspaceOriginal: orig,
      request: { endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies },
      rawContent: null,
      rawOriginal: null,
      address: 'whatever-was-open-before:4770',
      addressTouched: false,
    });
    expect(isRequestDirty(useStore.getState())).toBe(false);
  });
});

describe('what a save writes for headers', () => {
  it('leaves out the row that has no name yet', () => {
    useStore.setState({
      collectionParsed: makeParsed(),
      workspaceOriginal: makeParsed(),
      request: { endpoint: 'a.A/One', headers: { '': 'x', 'x-mark': '1' }, bodies: ['{}'] },
      address: '',
      addressTouched: false,
    });
    expect(structuredSave(useStore.getState()).headers).toEqual({ 'x-mark': '1' });
  });

  it('says nothing at all when every row is blank', () => {
    useStore.setState({ request: { endpoint: 'a.A/One', headers: { '': '' }, bodies: ['{}'] } });
    expect(structuredSave(useStore.getState()).headers).toBeUndefined();
  });
});

describe('removing META', () => {
  it('writes no META once every field is cleared, links included', () => {
    useStore.setState({
      collectionParsed: makeParsed({
        meta_name: 'login',
        meta_tags: ['smoke'],
        meta_links: ['https://tickets/1'],
      }),
    });
    const st = useStore.getState();
    st.setMetaField('meta_name', '');
    st.setMetaTags([]);
    st.setMetaLinks([]);
    expect(structuredSave(useStore.getState()).meta).toEqual({});
  });

  it('keeps the links a file carries until they are cleared', () => {
    useStore.setState({ collectionParsed: makeParsed({ meta_links: ['https://tickets/1'] }) });
    expect(structuredSave(useStore.getState()).meta).toEqual({ links: ['https://tickets/1'] });
  });
});

describe('a tab that is only text', () => {
  const tab = (over: any) => ({
    id: 't1', label: 'SayHello.gctf', endpoint: 'a.A/One', headers: {}, bodies: ['{}'],
    response: null, requestTab: 'source', gctfTab: 'request', responseTab: 'response',
    collectionPath: null, collectionParsed: null, collectionOriginal: null,
    rawContent: null, rawOriginal: null, ...over,
  }) as any;

  it('keeps its text across a reload and reopens on the source', () => {
    const stored = serializeTab(tab({ rawContent: '--- ENDPOINT ---\na.A/One\n' }));
    expect(stored.r).toContain('ENDPOINT');
    const back = deserializeTab(stored);
    expect(back.rawContent).toContain('ENDPOINT');
    expect(back.rawOriginal).toBeNull();
    expect(back.requestTab).toBe('source');
  });

  it('does not store the text of a file that is already on disk', () => {
    expect(serializeTab(tab({ collectionPath: 'a/b.gctf', rawContent: 'x' })).r).toBeUndefined();
  });

  it('refuses to store more text than the quota can carry', () => {
    expect(serializeTab(tab({ rawContent: 'x'.repeat(MAX_STORED_RAW + 1) })).r).toBeUndefined();
    expect(serializeTab(tab({ rawContent: 'x'.repeat(MAX_STORED_RAW) })).r).toHaveLength(MAX_STORED_RAW);
  });
});

describe('a request the workbench would not send', () => {
  async function refuse(): Promise<void> {
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string) => (String(url).includes('/api/call')
      ? { ok: false, status: 400, statusText: 'Bad Request', text: async () => 'message #1 is not valid JSON' }
      : { ok: true, status: 200, text: async () => '{}', json: async () => ({}) })) as unknown as typeof fetch;
    try {
      await useStore.getState().execute();
    } finally {
      globalThis.fetch = original;
    }
  }

  it('is not a failed call', async () => {
    useStore.setState({ request: { endpoint: 'a.B/C', headers: {}, bodies: ['{'] } });
    await refuse();

    const answer = useStore.getState().response;
    expect(answer?.sent).toBe(false);
    expect(answer?.durationMs).toBeNull();
    expect(answer?.error).toContain('not valid JSON');
  });

  it('leaves no history behind and counts against nothing', async () => {
    useStore.setState({ request: { endpoint: 'a.B/C', headers: {}, bodies: ['{'] } });
    const before = useStore.getState();
    const calls = before.history.length;
    const failures = before.totalError;
    await refuse();

    expect(useStore.getState().history.length).toBe(calls);
    expect(useStore.getState().totalError).toBe(failures);
  });
});

describe('the verb a tab is armed with', () => {
  const tab = (over: any) => ({
    id: 't1', label: 'check.gctf', endpoint: 'a.A/One', headers: {}, bodies: ['{}'],
    response: null, requestTab: 'body', gctfTab: 'request', responseTab: 'response',
    collectionPath: 'check.gctf', collectionParsed: null, collectionOriginal: null,
    rawContent: null, rawOriginal: null, ...over,
  }) as any;

  it('survives a reload', () => {
    const stored = serializeTab(tab({ runMode: 'run' }));
    expect(stored.m).toBe('run');
    expect(deserializeTab(stored).runMode).toBe('run');
  });

  it('costs nothing to store when it is the ordinary one', () => {
    expect(serializeTab(tab({ runMode: 'execute' })).m).toBeUndefined();
    expect(deserializeTab(serializeTab(tab({}))).runMode).toBe('execute');
  });

  it('belongs to the tab, not to the workbench', () => {
    const store = useStore.getState();
    store.setRunMode('run');
    expect(useStore.getState().runMode).toBe('run');
    expect(useStore.getState().tabs.find(t => t.id === useStore.getState().activeTabId)?.runMode).toBe('run');
    useStore.getState().setRunMode('execute');
  });
});

describe('the tab strip at its cap', () => {
  const clean = (n: number) => ({
    id: `t${n}`, label: `f${n}.gctf`, endpoint: 'a.A/One', headers: {}, bodies: ['{}'],
    response: null, requestTab: 'body', gctfTab: 'request', responseTab: 'response',
    collectionPath: `f${n}.gctf`, collectionParsed: null, collectionOriginal: null,
    rawContent: null, rawOriginal: null,
  }) as any;

  it('drops the oldest clean tab to make room', () => {
    const tabs = Array.from({ length: 50 }, (_, i) => clean(i));
    useStore.setState({ tabs, activeTabId: 't49' });
    const opened = useStore.getState().addTab({ label: 'new.gctf' });
    expect(opened).not.toBeNull();
    const after = useStore.getState().tabs;
    expect(after).toHaveLength(50);
    expect(after.some(t => t.id === 't0')).toBe(false);
    expect(after.some(t => t.id === 't49')).toBe(true);
  });

  it('refuses, and says it refused, when every tab is dirty', () => {
    const dirty = Array.from({ length: 50 }, (_, i) => ({
      ...clean(i), rawContent: 'edited', rawOriginal: 'loaded',
    }));
    useStore.setState({ tabs: dirty, activeTabId: 't0' });
    expect(useStore.getState().addTab({ label: 'new.gctf' })).toBeNull();
    expect(useStore.getState().tabs).toHaveLength(50);
  });
});

describe('moving a tab in the strip', () => {
  it('keeps the order the user dragged it into, and remembers it', () => {
    const store = useStore.getState();
    store.addTab();
    store.addTab();
    const before = useStore.getState().tabs.map(t => t.id);
    expect(before.length).toBeGreaterThanOrEqual(3);

    useStore.getState().moveTab(0, before.length - 1);
    const after = useStore.getState().tabs.map(t => t.id);
    expect(after[after.length - 1]).toBe(before[0]);
    expect(new Set(after)).toEqual(new Set(before));

  });

  it('leaves the strip alone when the drag ended nowhere', () => {
    const before = useStore.getState().tabs;
    useStore.getState().moveTab(0, 99);
    expect(useStore.getState().tabs).toBe(before);
  });
});

describe('a file with no request body', () => {
  it('opens an http file with no body and a gctf file with one', async () => {
    const parsed = makeParsed({ endpoint: 'GET /x', bodies: [] });
    globalThis.fetch = (async (url: string) => {
      if (String(url).startsWith('/api/collections/')) {
        return new Response(
          JSON.stringify({ content: '', parsed, documents: [], version: { mtime_ms: 1, hash: 'h' } }),
          { status: 200 },
        );
      }
      return new Response('[]', { status: 200 });
    }) as typeof fetch;

    await useStore.getState().loadCollection('probe.httf');
    expect(useStore.getState().request.bodies).toEqual([]);

    await useStore.getState().loadCollection('probe.gctf');
    expect(useStore.getState().request.bodies).toEqual(['{}']);
  });
});

describe('the curl a request writes out', () => {
  it('is the call the panel would make', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      address: 'http://127.0.0.1:8099',
      addressTouched: true,
      collectionParsed: null,
      request: {
        endpoint: 'POST /v1/users',
        headers: { 'content-type': 'application/json' },
        bodies: ['{"name":"Ada"}'],
      },
    });
    expect(useStore.getState().getCurlCommand())
      .toBe(`curl -L -X POST 'http://127.0.0.1:8099/v1/users' -H 'content-type: application/json' -d '{"name":"Ada"}'`);
  });

  it('dials the file\'s own address when the header was never typed', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      address: '',
      addressTouched: false,
      collectionParsed: makeParsed({ address: 'https://api.example.com', endpoint: 'GET /health' }),
      request: { endpoint: 'GET /health', headers: {}, bodies: [] },
    });
    expect(useStore.getState().getCurlCommand()).toBe(`curl -L 'https://api.example.com/health'`);
  });
});

describe('expect this, for an HTTP answer', () => {
  it('writes the status as an assertion and the body as the expected response', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      collectionParsed: makeParsed({ endpoint: 'GET /x', asserts: [] }),
      workspaceOriginal: null,
      request: { endpoint: 'GET /x', headers: {}, bodies: [] },
      response: {
        status: 'ok', statusCode: 201, messages: [{ id: 'u-1' }], headers: {}, trailers: {},
        error: null, durationMs: 3,
      },
    });
    expect(useStore.getState().expectFromResponse()).toBe(true);
    const p = useStore.getState().collectionParsed!;
    expect(p.asserts[0]).toBe('@status() == 201');
    expect(p.expect_responses[0].body).toContain('"id": "u-1"');
    expect(p.expect_error).toBeNull();
  });

  it('keeps a text body as the text it is', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      collectionParsed: makeParsed({ endpoint: 'GET /text', asserts: [] }),
      workspaceOriginal: null,
      request: { endpoint: 'GET /text', headers: {}, bodies: [] },
      response: {
        status: 'ok', statusCode: 200, messages: ['plain words here'], headers: {}, trailers: {},
        error: null, durationMs: 3,
      },
    });
    expect(useStore.getState().expectFromResponse()).toBe(true);
    expect(useStore.getState().collectionParsed!.expect_responses[0].body).toBe('plain words here');
  });

  it('replaces a status line it wrote before instead of stacking them', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      collectionParsed: makeParsed({ endpoint: 'GET /x', asserts: ['@status() == 200', '.ok == true'] }),
      workspaceOriginal: null,
      request: { endpoint: 'GET /x', headers: {}, bodies: [] },
      response: {
        status: 'ok', statusCode: 404, messages: [], headers: {}, trailers: {},
        error: null, durationMs: 3,
      },
    });
    useStore.getState().expectFromResponse();
    expect(useStore.getState().collectionParsed!.asserts).toEqual(['@status() == 404', '.ok == true']);
  });
});

describe('the default body of a new tab', () => {
  it('goes away when the request becomes an HTTP one', () => {
    useStore.setState({
      workspacePath: null,
      request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] },
    });
    useStore.getState().setEndpoint('POST /echo');
    expect(useStore.getState().request.bodies).toEqual([]);
  });

  it('leaves a body that was typed alone', () => {
    useStore.setState({
      workspacePath: null,
      request: { endpoint: 'a.B/C', headers: {}, bodies: ['{"a":1}'] },
    });
    useStore.getState().setEndpoint('POST /echo');
    expect(useStore.getState().request.bodies).toEqual(['{"a":1}']);
  });

  it('leaves a file alone — its REQUEST says what to send', () => {
    useStore.setState({
      workspacePath: 'x.httf',
      request: { endpoint: 'POST /echo', headers: {}, bodies: ['{}'] },
    });
    useStore.getState().setEndpoint('POST /other');
    expect(useStore.getState().request.bodies).toEqual(['{}']);
  });
});

describe('a tab restored from storage', () => {
  it('does not bring back a gRPC default body for an HTTP file', () => {
    expect(deserializeTab({ i: 't', l: 'p.httf', e: 'GET /x', h: {}, b: ['{}'], c: 'p.httf' }).bodies).toEqual([]);
  });

  it('keeps a body the file actually has', () => {
    expect(deserializeTab({ i: 't', l: 'p.httf', e: 'POST /x', h: {}, b: ['{"a":1}'], c: 'p.httf' }).bodies)
      .toEqual(['{"a":1}']);
  });

  it('leaves a gRPC tab as it was', () => {
    expect(deserializeTab({ i: 't', l: 'a.gctf', e: 'a.B/C', h: {}, b: ['{}'], c: 'a.gctf' }).bodies).toEqual(['{}']);
  });
});

describe('a file that is no longer there', () => {
  it('is known once the collections have been listed', () => {
    useStore.setState({
      workspacePath: 'gone.httf',
      collectionsRead: 'ok',
      collections: [{ path: 'here.httf', name: 'here', is_dir: false, tags: [] }],
    });
    expect(fileMissing(useStore.getState())).toBe(true);
  });

  it('is not claimed from a listing that never arrived', () => {
    useStore.setState({ workspacePath: 'gone.httf', collectionsRead: 'pending', collections: [] });
    expect(fileMissing(useStore.getState())).toBe(false);

    useStore.setState({ collectionsRead: 'failed' });
    expect(fileMissing(useStore.getState())).toBe(false);
  });

  it('is not claimed for a file that is there', () => {
    useStore.setState({
      workspacePath: 'here.httf',
      collectionsRead: 'ok',
      collections: [{ path: 'here.httf', name: 'here', is_dir: false, tags: [] }],
    });
    expect(fileMissing(useStore.getState())).toBe(false);
  });

  it('says nothing about a tab with no file', () => {
    useStore.setState({ workspacePath: null, collections: [] });
    expect(fileMissing(useStore.getState())).toBe(false);
  });
});

describe('opening a file that is not there', () => {
  it('answers that it did not open it', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async () => new Response('not found', { status: 404 })) as typeof fetch;
    try {
      useStore.setState({ tabs: [], activeTabId: '' });
      expect(await useStore.getState().loadCollection('gone.httf')).toBe(false);
      expect(useStore.getState().tabs).toEqual([]);
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('the source of a file that cannot be read', () => {
  it('says why instead of loading forever', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async () => new Response('gone', { status: 404 })) as typeof fetch;
    try {
      useStore.setState({ workspacePath: 'gone.httf', rawContent: null, rawError: null });
      await useStore.getState().loadRawContent();
      expect(useStore.getState().rawError).toContain('not in this workbench');
      expect(useStore.getState().rawContent).toBeNull();
    } finally {
      globalThis.fetch = original;
    }
  });

  it('says so when the workbench itself cannot be reached', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async () => { throw new Error('offline'); }) as typeof fetch;
    try {
      useStore.setState({ workspacePath: 'x.httf', rawContent: null, rawError: null });
      await useStore.getState().loadRawContent();
      expect(useStore.getState().rawError).toContain('could not be reached');
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('the project settings that arrive after the page has', () => {
  it('do not overwrite a connection chosen since boot', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string) => {
      if (String(url).includes('/api/info')) {
        return new Response(JSON.stringify({ status: 'ok', project: { active: true, project_dir: '.grpctestify', envs: [] } }));
      }
      if (String(url).includes('/api/project/settings')) {
        return new Response(JSON.stringify({ address: 'localhost:4770', protocol: 'grpc' }));
      }
      return new Response('null');
    }) as typeof fetch;
    try {
      useStore.setState({ address: '', addressTouched: false, protocolTouched: false });
      useStore.getState().setAddress('http://127.0.0.1:8899');
      await useStore.getState().loadStartupInfo();
      expect(useStore.getState().address).toBe('http://127.0.0.1:8899');
    } finally {
      globalThis.fetch = original;
    }
  });

  it('still aim a session that has chosen nothing', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string) => {
      if (String(url).includes('/api/info')) {
        return new Response(JSON.stringify({ status: 'ok', project: { active: true, project_dir: '.grpctestify', envs: [] } }));
      }
      if (String(url).includes('/api/project/settings')) {
        return new Response(JSON.stringify({ address: 'localhost:4770' }));
      }
      return new Response('null');
    }) as typeof fetch;
    try {
      useStore.setState({ address: '', addressTouched: false, protocolTouched: false });
      await useStore.getState().loadStartupInfo();
      expect(useStore.getState().address).toBe('localhost:4770');
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('the calls the status bar counts', () => {
  const answer = (status: number) => new Response(JSON.stringify({
    success: true, messages: [{}], grpc_status: status, headers: { ':status': String(status) },
    trailers: {}, error: null, shape: 'unary', messages_total: 1, messages_truncated: false,
  }), { headers: { 'content-type': 'application/json' } });

  const armed = (endpoint: string) => {
    useStore.setState({
      workspacePath: null, tabs: [], activeTabId: 'one',
      request: { endpoint, headers: {}, bodies: [] },
      totalOk: 0, totalError: 0, address: 'http://x.test',
    });
    useStore.setState({
      tabs: [{ ...useStore.getState().tabs[0] ?? {}, id: 'one', endpoint } as never],
      activeTabId: 'one',
    });
  };

  it('counts an HTTP failure as one', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async () => answer(404)) as typeof fetch;
    try {
      armed('GET /v1/users');
      await useStore.getState().execute();
      expect(useStore.getState().totalError).toBe(1);
      expect(useStore.getState().totalOk).toBe(0);
    } finally {
      globalThis.fetch = original;
    }
  });

  it('counts an answered 200 as a success', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async () => answer(200)) as typeof fetch;
    try {
      armed('GET /v1/users');
      await useStore.getState().execute();
      expect(useStore.getState().totalOk).toBe(1);
      expect(useStore.getState().totalError).toBe(0);
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('opening a line a run wrote', () => {
  it('opens the file it ran', async () => {
    const original = globalThis.fetch;
    const asked: string[] = [];
    globalThis.fetch = (async (url: string) => {
      asked.push(String(url));
      return new Response(JSON.stringify({
        parsed: { endpoint: 'GET /v1/users', address: '', headers: {}, bodies: [], asserts: [], extracts: {},
          meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
          tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [], expect_responses: [], expect_error: null },
        documents: [], content: '', version: null,
      }));
    }) as typeof fetch;
    try {
      useStore.setState({ tabs: [], activeTabId: '' });
      useStore.getState().restoreHistory({
        id: 'h1', timestamp: 1, endpoint: 'probe.httf', bodies: [], headers: {},
        kind: 'run', collectionPath: 'probe.httf',
        response: { status: 'ok', statusCode: 200, messages: [], headers: {}, trailers: {}, error: null, durationMs: 1 },
      });
      await new Promise(r => setTimeout(r, 0));
      expect(asked.some(u => u.includes('/api/collections/probe.httf'))).toBe(true);
      expect(useStore.getState().workspacePath).toBe('probe.httf');
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('a header row being typed', () => {
  it('is not sent with the call', async () => {
    const original = globalThis.fetch;
    let sent: Record<string, unknown> | undefined;
    globalThis.fetch = (async (_url: string, init?: RequestInit) => {
      sent = JSON.parse(String(init?.body ?? '{}'));
      return new Response(JSON.stringify({
        success: true, messages: [{}], grpc_status: 200, headers: {}, trailers: {},
        error: null, shape: 'unary', messages_total: 1, messages_truncated: false,
      }));
    }) as typeof fetch;
    try {
      useStore.setState({
        workspacePath: null, tabs: [], activeTabId: 'one', address: 'http://x.test',
        request: { endpoint: 'GET /x', headers: { '': '', authorization: 'Bearer t' }, bodies: [] },
      });
      await useStore.getState().execute();
      expect(sent?.headers).toEqual({ authorization: 'Bearer t' });
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('a run of the file that is open', () => {
  it('puts its own answer in that tab', async () => {
    useStore.setState({
      tabs: [{ ...deserializeTab({ i: 't1', l: 'a.gctf', e: 'a.B/C', h: {}, b: ['{}'], c: 'a.gctf' }) }],
      activeTabId: 't1',
      response: { status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: 'older', durationMs: 9 },
      run: { ...useStore.getState().run, verdicts: {}, cases: {} },
    });

    const events: unknown[] = [
      { event: 'test_pass', testId: 'a.gctf', duration: 4, assertions: [{ line: 3, expression: '.ok', passed: true }] },
    ];
    const { applyEvent } = await import('./jobs');
    useStore.setState(s => ({ run: applyEvent(s.run, events[0] as never) }));
    const verdict = useStore.getState().run.verdicts['a.gctf'];
    expect(verdict?.state).toBe('pass');
    const { verdictResult } = await import('./jobs');
    const answered = verdictResult(verdict);
    expect(answered?.status).toBe('ok');
    expect(answered?.fromRun).toBe(true);
    expect(answered?.assertions?.[0]?.passed).toBe(true);
  });
});

describe('a run started from the panel', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  const answer = (body: unknown, ok = true) =>
    (async () => ({ ok, status: 200, statusText: 'OK', text: async () => JSON.stringify(body) })) as never;

  it('marks the file the way a job does', async () => {
    useStore.setState({
      workspacePath: 'chain.httf', tabs: [], activeTabId: null,
      run: { ...useStore.getState().run, verdicts: {} },
    });
    globalThis.fetch = answer({
      success: true, error: null, grpc_status: 200, call_duration_ms: 4,
      assertions: [{ line: 8, expression: '@status() == 200', passed: true }],
      documents: [3, 1], response_messages: [{ name: 'Ada' }], headers: {}, trailers: {},
    });

    await useStore.getState().runTest();
    const v = useStore.getState().run.verdicts['chain.httf'];
    expect(v.state).toBe('pass');
    expect(v.documents).toEqual([3, 1]);
    expect(v.assertions).toHaveLength(1);
  });

  it('says which step the answer came from', async () => {
    useStore.setState({
      workspacePath: 'chain.httf', tabs: [], activeTabId: null, activeStep: 0,
      documents: [{ index: 0 }, { index: 1 }] as never,
      run: { ...useStore.getState().run, verdicts: {} },
    });
    globalThis.fetch = answer({
      success: true, error: null, grpc_status: 200, call_duration_ms: 4,
      assertions: [], documents: [3, 1], response_messages: [{ name: 'Ada' }], headers: {}, trailers: {},
    });

    await useStore.getState().runTest();
    expect(useStore.getState().response?.fromStep).toBe(1);
  });

  it('leaves a chain that stopped early on the step that stopped it', async () => {
    useStore.setState({
      workspacePath: 'chain.httf', tabs: [], activeTabId: null, activeStep: 2,
      documents: [{ index: 0 }, { index: 1 }, { index: 2 }] as never,
      run: { ...useStore.getState().run, verdicts: {} },
    });
    globalThis.fetch = answer({
      success: false, error: 'step 2 failed', grpc_status: 404, call_duration_ms: 1,
      assertions: [], documents: [3, 1], response_messages: [], headers: {}, trailers: {},
    });

    await useStore.getState().runTest();
    expect(useStore.getState().response?.fromStep).toBe(1);
  });

  it('marks a failure as one', async () => {
    useStore.setState({ workspacePath: 'chain.httf', tabs: [], activeTabId: null, run: { ...useStore.getState().run, verdicts: {} } });
    globalThis.fetch = answer({
      success: false, error: 'step 2 failed', grpc_status: 404, call_duration_ms: 2,
      assertions: [{ line: 17, expression: '@status() == 200', passed: false }],
      documents: [3], response_messages: [], headers: {}, trailers: {},
    });

    await useStore.getState().runTest();
    const v = useStore.getState().run.verdicts['chain.httf'];
    expect(v.state).toBe('fail');
    expect(v.message).toBe('step 2 failed');
  });

  it('names the file a run cannot read any more', async () => {
    useStore.setState({ workspacePath: 'api/probe.httf', tabs: [], activeTabId: null, run: { ...useStore.getState().run, verdicts: {} } });
    globalThis.fetch = (async () => ({ ok: false, status: 404, statusText: 'Not Found', text: async () => 'File not found' })) as never;

    await useStore.getState().runTest();
    expect(useStore.getState().response?.error).toBe('api/probe.httf is not on disk any more — Save writes this tab back to it');
  });

  it('does not leave the mark running when the run never lands', async () => {
    useStore.setState({ workspacePath: 'chain.httf', tabs: [], activeTabId: null, run: { ...useStore.getState().run, verdicts: {} } });
    globalThis.fetch = (async () => { throw new Error('the workbench is gone'); }) as never;

    await useStore.getState().runTest();
    expect(useStore.getState().run.verdicts['chain.httf'].state).toBe('fail');
  });
});

describe('where a call with nothing to aim it goes', () => {
  const bare = {
    collectionParsed: null, documents: [], activeStep: 0, address: '',
    activeEnvironment: null, environments: [], serverEnv: {}, protocol: 'grpc' as const,
  };

  it('is nowhere for an HTTP request', () => {
    useStore.setState({ ...bare, workspacePath: 'probe.httf', request: { endpoint: 'GET /v1/users', headers: {}, bodies: [] } } as never);
    expect(callAddress(useStore.getState())).toBe('');
  });

  it('is the transport default for a gRPC one', () => {
    useStore.setState({ ...bare, workspacePath: 'greet.gctf', request: { endpoint: 'pkg.Svc/M', headers: {}, bodies: ['{}'] } } as never);
    expect(callAddress(useStore.getState())).toBe('localhost:4770');
  });

  it('is whatever names it, either way', () => {
    useStore.setState({ ...bare, workspacePath: 'probe.httf', address: 'http://api:8899', request: { endpoint: 'GET /v1/users', headers: {}, bodies: [] } } as never);
    expect(callAddress(useStore.getState())).toBe('http://api:8899');
  });
});

describe('a tab that never read its file', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  const handleTab = (overrides: Partial<Tab> = {}): Tab => ({
    ...(useStore.getState().tabs[0] ?? ({} as Tab)),
    id: 'unread-1', label: 'probe.httf', endpoint: 'GET /x', headers: {}, bodies: [],
    response: null, requestTab: 'request', gctfTab: 'form', responseTab: 'response',
    collectionPath: 'api/probe.httf', collectionParsed: null, collectionOriginal: null,
    rawContent: null, rawOriginal: null, isPreview: false, addressTouched: false,
    ...overrides,
  } as Tab);

  it('is known for what it is, and its edits count as edits', async () => {
    globalThis.fetch = (async () => ({ ok: false, status: 500, json: async () => ({}) })) as never;
    useStore.setState({ tabs: [handleTab()], activeTabId: 'unread-1' } as never);
    useStore.setState(loadState('api/probe.httf'));

    await useStore.getState().hydrateStaleTabs();
    expect(contentUnread(useStore.getState())).toBe(true);

    expect(isRequestDirty(useStore.getState())).toBe(false);
    useStore.getState().setEndpoint('GET /y');
    expect(isRequestDirty(useStore.getState())).toBe(true);
  });

  it('is not a tab that read it', async () => {
    globalThis.fetch = (async () => ({
      ok: true,
      json: async () => ({ parsed: { ...makeParsed({ endpoint: 'GET /x' }) }, documents: [], version: null }),
    })) as never;
    useStore.setState({ tabs: [handleTab({ id: 'read-1' })], activeTabId: 'read-1' } as never);
    useStore.setState(loadState('api/probe.httf'));

    await useStore.getState().hydrateStaleTabs();
    expect(contentUnread(useStore.getState())).toBe(false);
  });

  it('is not an unsaved draft', () => {
    useStore.setState({ workspacePath: null, rawContent: null } as never);
    expect(contentUnread(useStore.getState())).toBe(false);
  });
});

function loadState(path: string) {
  return { workspacePath: path, rawContent: null, workspaceOriginal: null, collectionParsed: null } as never;
}

describe('a file the parser could not read', () => {
  it('is held by its text, not by the forms', () => {
    useStore.setState({
      workspacePath: 'badmeta.httf',
      parseError: 'Invalid META: tags: invalid type',
      rawContent: '--- META ---\ntags: a, b\n',
      rawOriginal: '--- META ---\ntags: a, b\n',
    } as never);
    expect(rawIsAuthoritative(useStore.getState())).toBe(true);
  });

  it('is the ordinary rule for a file that parses', () => {
    useStore.setState({
      workspacePath: 'fine.gctf',
      parseError: null,
      rawContent: '--- ENDPOINT ---\na.B/C\n',
      rawOriginal: '--- ENDPOINT ---\na.B/C\n',
    } as never);
    expect(rawIsAuthoritative(useStore.getState())).toBe(false);
  });
});

describe('expecting the answer of a chain', () => {
  const step = (index: number, endpoint: string) => ({
    index, endpoint, kind: 'unary' as const, address: '', address_source: 'inherited' as const,
    headers: {}, bodies: [], asserts: [], extracts: {}, options: {}, tls: {}, proto: {},
    produces: [], consumes: [],
  });

  it('writes it into the step that answered', () => {
    const head = makeParsed({ endpoint: 'GET /a', bodies: [] });
    useStore.setState({
      workspacePath: 'chain.httf',
      documents: [step(0, 'GET /a'), step(1, 'GET /b')] as never,
      activeStep: 0,
      headParsed: head,
      collectionParsed: head,
      workspaceOriginal: head,
      request: { endpoint: 'GET /a', headers: {}, bodies: [] },
      response: {
        status: 'ok', statusCode: 200, messages: [{ name: 'Ada' }], headers: {}, trailers: {},
        error: null, durationMs: 1, fromStep: 1,
      },
    } as never);

    expect(useStore.getState().expectFromResponse()).toBe(true);
    expect(useStore.getState().activeStep).toBe(1);
  });

  it('moves to the answering step before anything is written from it', () => {
    const head = makeParsed({ endpoint: 'GET /a', bodies: [] });
    useStore.setState({
      workspacePath: 'chain.httf',
      documents: [step(0, 'GET /a'), step(1, 'GET /b')] as never,
      activeStep: 0,
      headParsed: head,
      collectionParsed: head,
      workspaceOriginal: head,
      request: { endpoint: 'GET /a', headers: {}, bodies: [] },
      response: {
        status: 'ok', statusCode: 200, messages: [{ name: 'Ada' }], headers: {}, trailers: {},
        error: null, durationMs: 1, fromStep: 1,
      },
    } as never);

    expect(useStore.getState().focusAnswerStep()).toBe(true);
    expect(useStore.getState().activeStep).toBe(1);
  });

  it('refuses when the step on screen has edits that moving would lose', () => {
    const head = makeParsed({ endpoint: 'GET /a', bodies: [] });
    useStore.setState({
      workspacePath: 'chain.httf',
      documents: [step(0, 'GET /a'), step(1, 'GET /b')] as never,
      activeStep: 0,
      headParsed: head,
      collectionParsed: head,
      workspaceOriginal: head,
      request: { endpoint: 'GET /edited', headers: {}, bodies: [] },
      response: {
        status: 'ok', statusCode: 200, messages: [{}], headers: {}, trailers: {},
        error: null, durationMs: 1, fromStep: 1,
      },
    } as never);

    expect(useStore.getState().focusAnswerStep()).toBe(false);
    expect(useStore.getState().activeStep).toBe(0);
  });

  it('leaves a single-document file where it is', () => {
    const only = makeParsed({ endpoint: 'GET /a', bodies: [] });
    useStore.setState({
      workspacePath: 'one.httf',
      documents: [step(0, 'GET /a')] as never,
      activeStep: 0,
      headParsed: only,
      collectionParsed: only,
      workspaceOriginal: only,
      request: { endpoint: 'GET /a', headers: {}, bodies: [] },
      response: {
        status: 'ok', statusCode: 200, messages: [{ name: 'Ada' }], headers: {}, trailers: {},
        error: null, durationMs: 1, fromStep: 0,
      },
    } as never);

    expect(useStore.getState().expectFromResponse()).toBe(true);
    expect(useStore.getState().activeStep).toBe(0);
  });
});

describe('what the save dialog previews', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('is the text itself when the text is what a save writes', async () => {
    const seen: string[] = [];
    globalThis.fetch = (async (url: string) => {
      seen.push(String(url));
      return { ok: true, json: async () => ({ content: '--- ENDPOINT ---\nold\n', version: null }) };
    }) as never;

    useStore.setState({
      workspacePath: 'broken.gctf',
      parseError: 'Invalid META',
      rawContent: '--- META ---\ntags: a, b\n',
      rawOriginal: '--- META ---\ntags: a, b\n',
    } as never);

    const preview = await useStore.getState().previewSave('broken.gctf');
    expect(preview.content).toBe('--- META ---\ntags: a, b\n');
    expect(preview.current).toBe('--- ENDPOINT ---\nold\n');
    expect(seen.some(u => u.includes('preview-structured'))).toBe(false);
  });
});

describe('Save As on a file the text holds', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('writes the text, not the forms', async () => {
    const seen: string[] = [];
    globalThis.fetch = (async (url: string) => {
      seen.push(String(url));
      return { ok: true, json: async () => ({ mtime_ms: 1, hash: 'h' }), text: async () => '' };
    }) as never;

    useStore.setState({
      workspacePath: 'broken.gctf',
      parseError: 'Invalid META',
      rawContent: '--- META ---\ntags: a, b\n',
      rawOriginal: '--- META ---\ntags: a, b\n',
      request: { endpoint: 'pkg.Svc/M', headers: {}, bodies: ['{}'] },
      collectionParsed: null,
      workspaceOriginal: null,
      tabs: [], activeTabId: null,
    } as never);

    await useStore.getState().saveWorkspaceAs('fixed.gctf');
    expect(seen.some(u => u.includes('/api/save-structured'))).toBe(false);
    expect(seen.some(u => u.endsWith('/api/save'))).toBe(true);
  });
});

describe('a command copied to be run elsewhere', () => {
  it('carries what the call would carry', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      collectionParsed: null,
      address: 'http://{{HOST}}:8899',
      addressTouched: true,
      activeEnvironment: 'dev',
      environments: [{ name: 'dev', source: 'browser', variables: { HOST: '127.0.0.1', TOKEN: 't0ken' } }],
      request: {
        endpoint: 'GET /v1/users/{{TOKEN}}',
        headers: { authorization: 'Bearer {{TOKEN}}' },
        bodies: [],
      },
    } as never);

    const line = useStore.getState().getCurlCommand();
    expect(line).toContain("'http://127.0.0.1:8899/v1/users/t0ken'");
    expect(line).toContain("-H 'authorization: Bearer t0ken'");
    expect(line).not.toContain('{{');
  });

  it('carries what the last run of this file bound', () => {
    useStore.setState({
      workspacePath: 'checkout.apif',
      collectionParsed: null,
      address: 'http://127.0.0.1:8899',
      addressTouched: true,
      activeEnvironment: null,
      environments: [],
      request: { endpoint: 'GET /v1/users/{{who}}', headers: { 'x-who': '{{who}}' }, bodies: [] },
      run: {
        ...useStore.getState().run,
        verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok']] } },
      },
    } as never);

    const line = useStore.getState().getCurlCommand();
    expect(line).toContain("'http://127.0.0.1:8899/v1/users/ok'");
    expect(line).toContain("-H 'x-who: ok'");
    expect(line).not.toContain('{{');
  });

  it('leaves another file\'s bindings out of it', () => {
    useStore.setState({
      workspacePath: 'other.httf',
      collectionParsed: null,
      address: 'http://127.0.0.1:8899',
      addressTouched: true,
      activeEnvironment: null,
      environments: [],
      request: { endpoint: 'GET /v1/users/{{who}}', headers: {}, bodies: [] },
      run: {
        ...useStore.getState().run,
        verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok']] } },
      },
    } as never);

    expect(useStore.getState().getCurlCommand()).toContain('{{who}}');
  });

  it('leaves a name the environment does not answer as written', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      collectionParsed: null,
      address: 'http://127.0.0.1:8899',
      addressTouched: true,
      activeEnvironment: null,
      environments: [],
      request: { endpoint: 'GET /v1/users', headers: { authorization: 'Bearer {{TOKEN}}' }, bodies: [] },
    } as never);

    expect(useStore.getState().getCurlCommand()).toContain('Bearer {{TOKEN}}');
  });
});

describe('a file that changed on disk', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  const serve = (parsed: CollectionParsed, mtime: number, content = '') => (async (url: string, init?: RequestInit) => {
    if (String(url).startsWith('/api/versions')) {
      const asked = JSON.parse(String(init?.body ?? '{"paths":[]}')).paths as string[];
      return {
        ok: true,
        json: async () => Object.fromEntries(asked.map(p => [p, { mtime_ms: mtime, hash: `h${mtime}` }])),
      };
    }
    return {
      ok: true,
      json: async () => ({ parsed, documents: [], content, version: { mtime_ms: mtime, hash: `h${mtime}` } }),
    };
  }) as never;

  it('is re-read into a tab with nothing unsaved', async () => {
    const before = makeParsed({ endpoint: 'pkg.Svc/Old' });
    globalThis.fetch = serve(before, 100);
    useStore.setState({ tabs: [], activeTabId: null, collections: [] } as never);
    await useStore.getState().loadCollection('a.gctf');

    const after = makeParsed({ endpoint: 'pkg.Svc/New' });
    globalThis.fetch = serve(after, 200);
    useStore.setState({ collections: [{ path: 'a.gctf', name: 'a', is_dir: false, tags: [], mtime_ms: 200 }] } as never);

    const changed = await useStore.getState().syncOpenFiles();
    expect(changed).toEqual(['a.gctf']);
    expect(useStore.getState().collectionParsed?.endpoint).toBe('pkg.Svc/New');
  });

  it('drops the verdict the old version earned', async () => {
    const before = makeParsed({ endpoint: 'pkg.Svc/Old' });
    globalThis.fetch = serve(before, 100);
    useStore.setState({ tabs: [], activeTabId: null, collections: [] } as never);
    await useStore.getState().loadCollection('c.gctf');
    useStore.setState(s => ({
      run: { ...s.run, verdicts: { 'c.gctf': { path: 'c.gctf', state: 'pass' as const } } },
    }));

    globalThis.fetch = serve(makeParsed({ endpoint: 'pkg.Svc/New' }), 200);
    useStore.setState({ collections: [{ path: 'c.gctf', name: 'c', is_dir: false, tags: [], mtime_ms: 200 }] } as never);
    await useStore.getState().syncOpenFiles();

    expect(useStore.getState().run.verdicts['c.gctf']).toBeUndefined();
  });

  it('is not taken from a tab that has edits — it is marked instead', async () => {
    const before = makeParsed({ endpoint: 'pkg.Svc/Old' });
    globalThis.fetch = serve(before, 100);
    useStore.setState({ tabs: [], activeTabId: null, collections: [] } as never);
    await useStore.getState().loadCollection('b.gctf');
    useStore.getState().setEndpoint('pkg.Svc/Edited');

    let read = 0;
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).startsWith('/api/versions')) {
        const asked = JSON.parse(String(init?.body ?? '{"paths":[]}')).paths as string[];
        return { ok: true, json: async () => Object.fromEntries(asked.map(p => [p, { mtime_ms: 200, hash: 'h200' }])) };
      }
      read += 1;
      return { ok: true, json: async () => ({}) };
    }) as never;

    const changed = await useStore.getState().syncOpenFiles();
    expect(changed).toEqual([]);
    expect(read).toBe(0);
    expect(useStore.getState().staleOnDisk).toBe(true);
  });
});

describe('formsAheadOfFile', () => {
  const base = () => ({ ...useStore.getState(), workspacePath: 'a.gctf' });

  it('is false with no file behind the tab', () => {
    const orig = makeParsed();
    expect(formsAheadOfFile({ ...base(), workspacePath: null, workspaceOriginal: orig,
      request: { ...useStore.getState().request, endpoint: 'other/Method' } } as any)).toBe(false);
  });

  it('is true when a form field has moved past the saved file', () => {
    const orig = makeParsed();
    expect(formsAheadOfFile({ ...base(), workspaceOriginal: orig,
      request: { ...useStore.getState().request, endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies } } as any)).toBe(false);
    expect(formsAheadOfFile({ ...base(), workspaceOriginal: orig,
      request: { ...useStore.getState().request, endpoint: 'other/Method', headers: orig.headers, bodies: orig.bodies } } as any)).toBe(true);
  });

  it('is false when the raw editor is the one holding the edits — a save writes its text', () => {
    const orig = makeParsed();
    expect(formsAheadOfFile({ ...base(), workspaceOriginal: orig,
      rawContent: '--- ENDPOINT ---\nother/Method\n', rawOriginal: '--- ENDPOINT ---\npkg.Svc/Method\n',
      request: { ...useStore.getState().request, endpoint: 'other/Method', headers: orig.headers, bodies: orig.bodies } } as any)).toBe(false);
  });
});

describe('projectCallEnv / resolveProjectAddress', () => {
  const env = (over: Partial<{ name: string; variables: Record<string, string>; mutedVariables: string[] }> = {}) => ({
    name: 'example', variables: { HOST: '127.0.0.1' }, source: 'project' as const, ...over,
  });

  it('is the project environment the server will use, not the one the picker shows', () => {
    const st = {
      ...useStore.getState(),
      projectEnvs: [env(), env({ name: 'staging', variables: { HOST: 'staging.internal' } })],
      projectDefaults: { address: '', protocol: 'grpc', tls: false, tlsInsecure: true, activeEnv: 'staging' },
      activeEnvironment: 'example',
    } as any;
    expect(projectCallEnv(st)?.variables.HOST).toBe('staging.internal');
  });

  it('has nothing to say without an active project environment', () => {
    const st = { ...useStore.getState(), projectEnvs: [env()], projectDefaults: null } as any;
    expect(projectCallEnv(st)).toBe(null);
  });

  it('drops a muted variable, the way a call does', () => {
    const st = {
      ...useStore.getState(),
      projectEnvs: [env({ mutedVariables: ['HOST'] })],
      projectDefaults: { address: '', protocol: 'grpc', tls: false, tlsInsecure: true, activeEnv: 'example' },
    } as any;
    expect(resolveProjectAddress('http://{{HOST}}:8899', projectCallEnv(st))).toBe('http://{{HOST}}:8899');
  });

  it('reads the address a call goes to', () => {
    expect(resolveProjectAddress('http://{{HOST}}:8899', env())).toBe('http://127.0.0.1:8899');
    expect(resolveProjectAddress('localhost:4770', env())).toBe('localhost:4770');
    expect(resolveProjectAddress('http://{{HOST}}:8899', null)).toBe('http://{{HOST}}:8899');
  });
});

describe('tabFileMissing', () => {
  const listing = new Set(['a.gctf', 'dir/b.httf']);

  it('says nothing before a listing has arrived', () => {
    expect(tabFileMissing(makeTab({ collectionPath: 'gone.gctf' }), null)).toBe(false);
  });

  it('marks a tab whose file the listing does not hold', () => {
    expect(tabFileMissing(makeTab({ collectionPath: 'gone.gctf' }), listing)).toBe(true);
    expect(tabFileMissing(makeTab({ collectionPath: 'a.gctf' }), listing)).toBe(false);
  });

  it('leaves an unsaved tab alone — it never had a file to lose', () => {
    expect(tabFileMissing(makeTab({ collectionPath: null }), listing)).toBe(false);
  });
});

describe('a command copied out of a project', () => {
  it('carries what the project resolves, not the braces', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      collectionParsed: null,
      address: 'http://{{HOST}}:8899',
      addressTouched: true,
      activeEnvironment: null,
      environments: [],
      projectEnvs: [{ name: 'example', source: 'project', variables: { HOST: '127.0.0.1', WHO: 'Ada' } }],
      projectDefaults: { address: '', protocol: 'grpc', tls: false, tlsInsecure: true, activeEnv: 'example' },
      request: {
        endpoint: 'GET /v1/{{WHO}}',
        headers: { 'x-who': '{{WHO}}' },
        bodies: [],
      },
    } as never);

    const line = useStore.getState().getCurlCommand();
    expect(line).toContain("'http://127.0.0.1:8899/v1/Ada'");
    expect(line).toContain("-H 'x-who: Ada'");
    expect(line).not.toContain('{{');
  });

  it('leaves the braces alone when no project environment is active', () => {
    useStore.setState({
      projectEnvs: [],
      projectDefaults: null,
      request: { endpoint: 'GET /v1/{{WHO}}', headers: {}, bodies: [] },
    } as never);
    expect(useStore.getState().getCurlCommand()).toContain('{{WHO}}');
  });
});

describe('two reflections in flight', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  const service = (name: string, method: string) => ({
    services: [{ name, methods: [{ name: method, full_name: `${name}/${method}`, client_streaming: false, server_streaming: false }] }],
  });

  it('does not fill the list from the server left behind', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    let call = 0;
    globalThis.fetch = (async () => {
      call += 1;
      if (call === 1) {
        return {
          ok: true, status: 200, statusText: 'OK',
          json: async () => { await slow; return service('old.Svc', 'Left'); },
        } as never;
      }
      return { ok: true, status: 200, statusText: 'OK', json: async () => service('new.Svc', 'Here') } as never;
    }) as never;

    useStore.setState({
      workspacePath: null, collectionParsed: null, documents: [],
      address: 'old:1', addressTouched: true, reflectionMethods: [],
    } as never);
    const first = useStore.getState().reflect();

    useStore.setState({ address: 'new:2' } as never);
    await useStore.getState().reflect();
    expect(useStore.getState().reflectionMethods.map(m => m.fullName)).toEqual(['new.Svc/Here']);

    release!();
    await first;
    expect(useStore.getState().reflectionMethods.map(m => m.fullName)).toEqual(['new.Svc/Here']);
  });
});

describe('a source read that comes back after the tabs were switched', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('lands in the tab that asked for it', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    globalThis.fetch = (async () => ({
      ok: true,
      status: 200,
      json: async () => { await slow; return { content: 'A on disk', version: { mtime_ms: 1, hash: 'h' } }; },
    })) as never;

    const a = makeTab({ id: 'A', collectionPath: 'a.gctf', rawContent: null, rawOriginal: null });
    const b = makeTab({ id: 'B', collectionPath: 'b.gctf', rawContent: null, rawOriginal: null });
    useStore.setState({
      tabs: [a, b], activeTabId: 'A', workspacePath: 'a.gctf',
      rawContent: null, rawOriginal: null,
    } as never);

    const reading = useStore.getState().loadRawContent();
    useStore.getState().setActiveTab('B');
    release!();
    await reading;

    const state = useStore.getState();
    expect(state.activeTabId).toBe('B');
    expect(state.rawContent).toBe(null);
    expect(state.tabs.find(t => t.id === 'B')!.rawContent).toBe(null);
    expect(state.tabs.find(t => t.id === 'A')!.rawContent).toBe('A on disk');
  });
});

describe('two files clicked one after the other', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  const file = (endpoint: string) => ({
    content: `--- ENDPOINT ---\n${endpoint}\n`,
    version: { mtime_ms: 1, hash: 'h' },
    parsed: { ...makeParsed({ endpoint }) },
    documents: [],
  });

  it('shows the one asked for last, and keeps both', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    globalThis.fetch = (async (url: string) => ({
      ok: true,
      status: 200,
      json: async () => {
        if (url.endsWith('slow.gctf')) { await slow; return file('slow.Svc/M'); }
        return file('quick.Svc/M');
      },
    })) as never;

    useStore.setState({ tabs: [], activeTabId: null, collections: [] } as never);
    const first = useStore.getState().loadCollection('slow.gctf');
    await useStore.getState().loadCollection('quick.gctf');
    release!();
    expect(await first).toBe(true);

    const state = useStore.getState();
    const active = state.tabs.find(t => t.id === state.activeTabId);
    expect(active?.collectionPath).toBe('quick.gctf');
    expect(state.tabs.map(t => t.collectionPath).sort()).toEqual(['quick.gctf', 'slow.gctf']);
  });
});

describe('a save that finishes after the tabs were switched', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('stamps the tab that was saved, not the one on screen', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    globalThis.fetch = (async (url: string) => {
      if (String(url).includes('/api/save-structured')) {
        await slow;
        return { ok: true, status: 200, json: async () => ({ mtime_ms: 2, hash: 'h2' }) } as never;
      }
      return { ok: true, status: 200, json: async () => ({ paths: [] }) } as never;
    }) as never;

    const savedParsed = makeParsed({ endpoint: 'a.Svc/One' });
    const otherParsed = makeParsed({ endpoint: 'b.Svc/Two' });
    const a = makeTab({ id: 'A', collectionPath: 'a.gctf', collectionParsed: savedParsed, collectionOriginal: savedParsed });
    const b = makeTab({ id: 'B', collectionPath: 'b.gctf', collectionParsed: otherParsed, collectionOriginal: otherParsed, endpoint: 'b.Svc/Two' });
    useStore.setState({
      tabs: [a, b], activeTabId: 'A', workspacePath: 'a.gctf',
      collectionParsed: savedParsed, workspaceOriginal: savedParsed,
      rawContent: null, rawOriginal: null,
      request: { endpoint: 'a.Svc/One-edited', headers: {}, bodies: ['{}'] },
    } as never);

    const saving = useStore.getState().saveWorkspace();
    useStore.getState().setActiveTab('B');
    release!();
    expect(await saving).toBe(true);

    const state = useStore.getState();
    expect(state.tabs.find(t => t.id === 'A')!.collectionOriginal!.endpoint).toBe('a.Svc/One-edited');
    expect(state.tabs.find(t => t.id === 'B')!.collectionOriginal!.endpoint).toBe('b.Svc/Two');
    expect(state.workspaceOriginal!.endpoint).toBe('b.Svc/Two');
  });
});

describe('Save As while the tabs are switched', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('renames the tab that was saved', async () => {
    let release: (() => void) | null = null;
    const slow = new Promise<void>(r => { release = r; });
    globalThis.fetch = (async (url: string) => {
      if (String(url).includes('/api/save-structured')) {
        await slow;
        return { ok: true, status: 200, json: async () => ({ mtime_ms: 3, hash: 'h3' }) } as never;
      }
      return { ok: true, status: 200, json: async () => ({ paths: [] }) } as never;
    }) as never;

    const a = makeParsed({ endpoint: 'a.Svc/One' });
    const b = makeParsed({ endpoint: 'b.Svc/Two' });
    useStore.setState({
      tabs: [
        makeTab({ id: 'A', collectionPath: 'a.gctf', collectionParsed: a, collectionOriginal: a }),
        makeTab({ id: 'B', collectionPath: 'b.gctf', collectionParsed: b, collectionOriginal: b }),
      ],
      activeTabId: 'A', workspacePath: 'a.gctf', collectionParsed: a, workspaceOriginal: a,
      rawContent: null, rawOriginal: null, collections: [],
      request: { endpoint: 'a.Svc/One', headers: {}, bodies: ['{}'] },
    } as never);

    const saving = useStore.getState().saveWorkspaceAs('copy');
    useStore.getState().setActiveTab('B');
    release!();
    await saving;

    const state = useStore.getState();
    expect(state.tabs.find(t => t.id === 'A')!.collectionPath).toBe('copy.gctf');
    expect(state.tabs.find(t => t.id === 'B')!.collectionPath).toBe('b.gctf');
    expect(state.workspacePath).toBe('b.gctf');
  });
});

describe('tabs from another workbench', () => {
  it('keeps what was typed and drops what belongs to the other directory', () => {
    const kept = keepFromAnotherRoot([
      makeTab({ id: '1', collectionPath: 'other/a.gctf' }),
      makeTab({ id: '2', collectionPath: null, endpoint: 'a.Svc/Typed', bodies: [''] }),
      makeTab({ id: '3', collectionPath: null, endpoint: '', bodies: [''], rawContent: null }),
    ]);
    expect(kept.map(t => t.id)).toEqual(['2']);
  });

  it('keeps a draft whose body was typed but whose endpoint was not', () => {
    const kept = keepFromAnotherRoot([
      makeTab({ id: '1', collectionPath: null, endpoint: '', bodies: ['{"a":1}'] }),
    ]);
    expect(kept.map(t => t.id)).toEqual(['1']);
  });
});

describe('which file a measurement is of', () => {
  it('is remembered when the bench starts', async () => {
    const originalFetch = globalThis.fetch;
    class FakeSource { close() {} addEventListener() {} }
    (globalThis as never as { EventSource: unknown }).EventSource = FakeSource;
    globalThis.fetch = (async () => ({ ok: true, status: 200, json: async () => ({ id: 'j1' }) })) as never;
    useStore.setState({ run: { ...useStore.getState().run, kind: 'run' }, benchPaths: [] } as never);
    await useStore.getState().startBench(['a.gctf', 'b.gctf']);
    expect(useStore.getState().benchPaths).toEqual(['a.gctf', 'b.gctf']);
    globalThis.fetch = originalFetch;
  });
});

describe('a save while the workbench is gone', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it("says so in the workbench's own words", async () => {
    globalThis.fetch = (async () => { throw new TypeError('Failed to fetch'); }) as never;
    const parsed = makeParsed();
    useStore.setState({
      tabs: [makeTab({ id: 'A', collectionPath: 'a.gctf', collectionParsed: parsed, collectionOriginal: parsed })],
      activeTabId: 'A', workspacePath: 'a.gctf', collectionParsed: parsed, workspaceOriginal: parsed,
      rawContent: null, rawOriginal: null, collections: [],
      request: { endpoint: 'a.Svc/One', headers: {}, bodies: ['{}'] },
    } as never);

    await expect(useStore.getState().saveWorkspace()).rejects.toThrow(/could not be reached/);
  });
});

describe('a project environment that cannot be written', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('says what the server said', async () => {
    globalThis.fetch = (async () => ({
      ok: false, status: 404, statusText: 'Not Found', text: async () => 'Not in project mode',
    })) as never;
    await expect(useStore.getState().saveProjectEnv('staging', 'A=1\n'))
      .rejects.toThrow('Not in project mode');
  });

  it('says the workbench is gone when it is', async () => {
    globalThis.fetch = (async () => { throw new TypeError('Failed to fetch'); }) as never;
    await expect(useStore.getState().deleteProjectEnv('staging'))
      .rejects.toThrow(/could not be reached/);
  });

  it('falls back to naming the file when the server said nothing', async () => {
    globalThis.fetch = (async () => ({
      ok: false, status: 500, statusText: 'Internal Server Error', text: async () => '   ',
    })) as never;
    await expect(useStore.getState().saveProjectEnvLocal('staging', 'A=1\n'))
      .rejects.toThrow('.env.staging.local could not be written');
  });
});

describe('an ask while the workbench is gone', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('says what did not happen, not what the browser calls it', async () => {
    globalThis.fetch = (async () => { throw new TypeError('Failed to fetch'); }) as never;
    useStore.setState({ request: { endpoint: 'a.Svc/One', headers: {}, bodies: ['{}'] } } as never);
    await expect(useStore.getState().scaffoldTest()).rejects.toThrow(/nothing was scaffolded/);
    await expect(useStore.getState().getGrpcurlCommand()).rejects.toThrow(/the command was not built/);
  });

  it('reads the server’s refusal of a command it would not build', async () => {
    globalThis.fetch = (async () => ({
      ok: false, status: 400, statusText: 'Bad Request', text: async () => 'endpoint must be service/method',
    })) as never;
    await expect(useStore.getState().getGrpcurlCommand()).rejects.toThrow('endpoint must be service/method');
  });
});

describe('the row a run answered from', () => {
  it('drives the panel’s run over the source the rail is set to', async () => {
    let sent: any = null;
    useStore.setState({ workspacePath: 'over.httf', runJobId: null, sessionId: '', runData: 'paths.csv' });
    globalThis.fetch = (async (_url: string, init: any) => {
      sent = JSON.parse(init.body);
      return {
        ok: true, status: 200, statusText: 'OK',
        text: async () => JSON.stringify({ success: true, assertions: [], response_messages: [], headers: {}, trailers: {}, documents: [1], row: 0, rows_total: 2 }),
      };
    }) as never;

    await useStore.getState().runTest();
    expect(sent.data).toBe('paths.csv');
  });

  it('records where the run went', async () => {
    useStore.setState({ workspacePath: 'a.gctf', runJobId: null, sessionId: '', runData: null, lastCallAddress: null });
    globalThis.fetch = (async () => ({
      ok: true, status: 200, statusText: 'OK',
      text: async () => JSON.stringify({
        success: true, assertions: [], response_messages: [], headers: {}, trailers: {},
        documents: [1], address: 'localhost:50051',
      }),
    })) as never;

    await useStore.getState().runTest();
    expect(useStore.getState().lastCallAddress).toBe('localhost:50051');
  });

  it('is on the answer the panel shows', async () => {
    const state = useStore.getState();
    useStore.setState({
      workspacePath: 'rows.httf',
      tabs: [],
      activeTabId: null,
      runJobId: null,
      sessionId: '',
      runData: null,
    });
    globalThis.fetch = (async () => ({
      ok: true,
      status: 200,
      statusText: 'OK',
      text: async () => JSON.stringify({
        success: false,
        error: 'Could not reach http://127.0.0.1:1/data.json',
        assertions: [],
        response_messages: [],
        headers: {},
        trailers: {},
        documents: [1],
        row: 1,
        rows_total: 2,
      }),
    })) as never;

    await state.runTest();
    expect(useStore.getState().response?.fromCase).toBe('row 2 of 2');
  });
});

describe('the version a conflict offers to keep', () => {
  it('is what the server would write', async () => {
    const parsed = {
      endpoint: 'a.Svc/One', address: '', headers: {}, bodies: ['{}'], asserts: ['.ok == true'],
      extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null,
      meta_links: [], tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
      expect_responses: [], expect_error: null,
    } as unknown as CollectionParsed;

    useStore.setState({
      workspacePath: 'a.gctf',
      request: { endpoint: 'a.Svc/Two', headers: {}, bodies: ['{}'] },
      collectionParsed: parsed,
      workspaceOriginal: parsed,
      rawContent: null,
      rawOriginal: null,
      tabs: [],
      activeTabId: null,
      activeStep: 0,
    });

    const seen: string[] = [];
    globalThis.fetch = (async (url: string) => {
      seen.push(String(url));
      if (String(url).includes('preview-structured')) {
        return { ok: true, status: 200, json: async () => ({ content: 'the file the server would write' }) };
      }
      return {
        ok: false, status: 409, statusText: 'Conflict',
        text: async () => JSON.stringify({ error: 'conflict', content: 'what is on disk' }),
        json: async () => ({ error: 'conflict', content: 'what is on disk' }),
      };
    }) as never;

    await useStore.getState().saveWorkspace();
    expect(seen.some(u => u.includes('preview-structured'))).toBe(true);
    expect(useStore.getState().saveConflict?.mine).toBe('the file the server would write');
  });
});

describe('a file that moved', () => {
  it('takes the run’s data source with it', () => {
    useStore.setState({ runData: 'data/paths.csv', tabs: [], run: { ...useStore.getState().run, verdicts: {}, cases: {} } });
    useStore.getState().retargetPath('data/paths.csv', 'rows/paths.csv');
    expect(useStore.getState().runData).toBe('rows/paths.csv');
  });

  it('leaves another source alone', () => {
    useStore.setState({ runData: 'data/other.csv', tabs: [], run: { ...useStore.getState().run, verdicts: {}, cases: {} } });
    useStore.getState().retargetPath('data/paths.csv', 'rows/paths.csv');
    expect(useStore.getState().runData).toBe('data/other.csv');
  });
});

describe('a bench started over unsaved edits', () => {
  it('remembers which files the numbers will not be of', async () => {
    const fetchMock = vi.fn(async (url: unknown) => {
      if (String(url).includes('/api/jobs')) {
        return new Response(JSON.stringify({ id: 'j1', total: 1 }), { status: 200 });
      }
      return new Response('{}', { status: 200 });
    });
    vi.stubGlobal('fetch', fetchMock);
    vi.stubGlobal('EventSource', class { close() {} addEventListener() {} } as never);
    useStore.setState({
      tabs: [{ ...useStore.getState().tabs[0], collectionPath: 'a.gctf' }],
    } as never);
    const tab = useStore.getState().tabs[0];
    useStore.setState({ tabs: [{ ...tab, collectionPath: 'a.gctf', rawContent: 'edited', rawOriginal: 'on disk' }] } as never);
    await useStore.getState().startBench('a.gctf');
    expect(useStore.getState().benchOverUnsaved).toEqual(['a.gctf']);
    vi.unstubAllGlobals();
  });
});

describe('a call made from a file', () => {
  it('is still a request when it is opened, and says which file it came from', async () => {
    const original = globalThis.fetch;
    const asked: string[] = [];
    globalThis.fetch = (async (url: string) => {
      asked.push(String(url));
      return new Response('{}', { status: 200 });
    }) as typeof fetch;
    try {
      useStore.setState({ tabs: [], activeTabId: '' });
      useStore.getState().restoreHistory({
        id: 'h2', timestamp: 1, endpoint: 'echo.EchoService/SayHello', bodies: ['{}'], headers: {},
        collectionPath: 'echo.gctf',
        connection: { address: 'localhost:50051', tls: false },
        response: { status: 'ok', statusCode: 0, messages: [], headers: {}, trailers: {}, error: null, durationMs: 1 },
      });
      await new Promise(r => setTimeout(r, 0));
      expect(asked.some(u => u.includes('/api/collections/'))).toBe(false);
      expect(useStore.getState().request.endpoint).toBe('echo.EchoService/SayHello');
      expect(useStore.getState().address).toBe('localhost:50051');
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('the mark a file changing under a dirty tab leaves', () => {
  const listing = (mtime: number) => [{ path: 'a.gctf', name: 'a', is_dir: false, tags: [], mtime_ms: mtime }];

  it('goes when the tab and the file agree again', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string) => {
      if (String(url).includes('/api/collections/')) {
        return new Response(JSON.stringify({
          parsed: { endpoint: 'a.A/One', address: '', headers: {}, bodies: [], asserts: [], extracts: {},
            meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
            tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [], expect_responses: [], expect_error: null },
          documents: [], content: 'on disk', version: { mtime_ms: 5, hash: 'h5' },
        }));
      }
      return new Response(JSON.stringify(listing(5)));
    }) as typeof fetch;
    try {
      useStore.setState({
        tabs: [{ ...useStore.getState().tabs[0], id: 't1', collectionPath: 'a.gctf', staleOnDisk: true }],
        activeTabId: 't1',
        staleOnDisk: true,
        collections: listing(5),
      } as never);
      await useStore.getState().syncOpenFiles();
      expect(useStore.getState().tabs[0].staleOnDisk).toBe(false);
      expect(useStore.getState().staleOnDisk).toBe(false);
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('discarding what a tab holds', () => {
  const onDisk = makeParsed({ endpoint: 'a.A/One', asserts: ['.a == 1'] });
  const second = {
    index: 1, endpoint: 'a.A/Two', address: '', address_source: 'inherited', headers: {},
    bodies: ['{}'], asserts: ['.b == 2'], extracts: {}, options: {}, tls: {}, proto: {},
  };

  function armDirty(activeStep: number) {
    const tab = makeTab({ collectionPath: 'chain.gctf' });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'chain.gctf',
      request: { endpoint: 'a.A/Edited', headers: {}, bodies: ['{}'] },
      collectionParsed: onDisk, workspaceOriginal: onDisk, headParsed: onDisk,
      documents: [{ ...second, index: 0, endpoint: 'a.A/One', asserts: ['.a == 1'] }, second] as never,
      activeStep, rawContent: null, rawOriginal: null,
    } as never);
  }

  function serveTheFile() {
    globalThis.fetch = (async (url: string) => {
      if (String(url).includes('/api/collections/')) {
        return new Response(JSON.stringify({
          parsed: onDisk,
          documents: [{ ...second, index: 0, endpoint: 'a.A/One', asserts: ['.a == 1'] }, second],
          content: 'on disk', version: { mtime_ms: 1, hash: 'h' },
        }));
      }
      return new Response('[]');
    }) as typeof fetch;
  }

  it('reads the file again and the forms stop being ahead of it', async () => {
    const original = globalThis.fetch;
    armDirty(0);
    serveTheFile();
    try {
      expect(await useStore.getState().discardEdits()).toBe(true);
      expect(useStore.getState().request.endpoint).toBe('a.A/One');
      expect(isRequestDirty(useStore.getState())).toBe(false);
    } finally {
      globalThis.fetch = original;
    }
  });

  it('leaves the chain on the step that was open', async () => {
    const original = globalThis.fetch;
    armDirty(1);
    serveTheFile();
    try {
      expect(await useStore.getState().discardEdits()).toBe(true);
      expect(useStore.getState().activeStep).toBe(1);
      expect(useStore.getState().request.endpoint).toBe('a.A/Two');
    } finally {
      globalThis.fetch = original;
    }
  });

  it('has nothing to do for a tab that holds no edits', async () => {
    const tab = makeTab({ collectionPath: 'clean.gctf' });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'clean.gctf',
      request: { endpoint: onDisk.endpoint, headers: {}, bodies: onDisk.bodies },
      collectionParsed: onDisk, workspaceOriginal: onDisk, rawContent: null, rawOriginal: null,
      activeStep: 0,
    } as never);
    expect(await useStore.getState().discardEdits()).toBe(false);
  });
});

describe('why the text is what a save writes', () => {
  it('names a file the parser could not read', () => {
    expect(rawAuthorityReason({ rawContent: 'x', rawOriginal: 'x', parseError: 'Invalid META' }))
      .toBe('unreadable');
  });

  it('names text with nothing on disk behind it', () => {
    expect(rawAuthorityReason({ rawContent: 'scaffolded', rawOriginal: null })).toBe('no-file');
  });

  it('names an edited source', () => {
    expect(rawAuthorityReason({ rawContent: 'edited', rawOriginal: 'loaded' })).toBe('edited');
  });

  it('says nothing when the forms are the ones a save writes', () => {
    expect(rawAuthorityReason({ rawContent: 'same', rawOriginal: 'same' })).toBeNull();
    expect(rawAuthorityReason({ rawContent: null, rawOriginal: null })).toBeNull();
  });

  it('has a sentence for each, where an action is refused', () => {
    expect(rawAuthorityRefusal('unreadable')).toContain('could not be read');
    expect(rawAuthorityRefusal('no-file')).toContain('no file behind it');
    expect(rawAuthorityRefusal('edited')).toContain('unsaved edits');
    expect(rawAuthorityRefusal(null)).toBeNull();
  });
});

describe('a run of the open file the server would not start', () => {
  afterEach(() => { vi.unstubAllGlobals(); });

  function armed(path: string | null) {
    const tab = makeTab({ id: 'tR', collectionPath: path, response: null });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'gone.gctf', response: null, runError: null,
    });
    useStore.setState({ collections: [{ path: 'gone.gctf', name: 'gone.gctf', is_dir: false }] as never, collectionsRead: 'ok' });
    globalThis.fetch = (async (url: string) => (
      url.startsWith('/api/collections')
        ? { ok: true, json: async () => [] }
        : { ok: false, status: 404, text: async () => 'File not found: gone.gctf' }
    )) as never;
  }

  it('answers in the pane that offered the run', async () => {
    armed('gone.gctf');
    await useStore.getState().startRun(['gone.gctf']);
    const st = useStore.getState();
    expect(st.response?.error).toBe('gone.gctf is not on disk any more — Save writes this tab back to it');
    expect(st.tabs[0].response?.error).toBe(st.response?.error);
    expect(st.runError).toBe(st.response?.error);
    expect(fileMissing(st)).toBe(true);
  });

  it('answers a refusal that names no file when the run was of this one', async () => {
    const tab = makeTab({ id: 'tR', collectionPath: 'rows.gctf', response: null });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'rows.gctf', response: null, runError: null,
      collections: [{ path: 'rows.gctf', name: 'rows.gctf', is_dir: false }] as never, collectionsRead: 'ok',
    } as never);
    globalThis.fetch = (async () => ({
      ok: false, status: 400,
      text: async () => 'rows.gctf has a DATASET section, which is its own row source — a data source cannot be combined with it',
    })) as never;

    await useStore.getState().startRun(['rows.gctf']);
    const st = useStore.getState();
    expect(st.response?.error).toContain('a data source cannot be combined with it');
    expect(st.runError).toBe(st.response?.error);
  });

  it('says nothing in the pane when the run was of several files', async () => {
    const tab = makeTab({ id: 'tM', collectionPath: 'rows.gctf', response: null });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, workspacePath: 'rows.gctf', response: null, runError: null,
    } as never);
    globalThis.fetch = (async () => ({ ok: false, status: 400, text: async () => 'The load runner measures gRPC calls — there is no .gctf file in this selection' })) as never;

    await useStore.getState().startRun(['a.gctf', 'b.httf']);
    expect(useStore.getState().response).toBe(null);
    expect(useStore.getState().runError).toContain('no .gctf file in this selection');
  });

  it('leaves another tab\'s answer alone', async () => {
    armed('other.gctf');
    await useStore.getState().startRun(['gone.gctf', 'other.gctf']);
    const st = useStore.getState();
    expect(st.response).toBe(null);
    expect(st.runError).toBe('gone.gctf is not on disk any more — it was renamed or deleted since the rail read it');
  });
});

describe('duplicating a file', () => {
  it('copies the bytes on disk and opens the copy', async () => {
    const original = globalThis.fetch;
    const written: any[] = [];
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).includes('/api/collections/') && !init) {
        return { ok: true, json: async () => ({ content: '--- ENDPOINT ---\npkg.Svc/M\n', parsed: null, version: { mtime_ms: 1, hash: 'a' } }) };
      }
      if (String(url) === '/api/save') {
        written.push(JSON.parse(String(init?.body)));
        return { ok: true, json: async () => ({ mtime_ms: 2, hash: 'b' }) };
      }
      return { ok: true, json: async () => ({}) };
    }) as any;
    useStore.setState({
      collections: [{ path: 'auth/login.gctf', name: 'login.gctf', is_dir: false } as any],
      collectionsRead: 'ok',
    });
    try {
      const name = await useStore.getState().duplicateCollection('auth/login.gctf');
      expect(name).toBe('auth/login-2.gctf');
      expect(written[0]).toMatchObject({ path: 'auth/login-2.gctf', content: '--- ENDPOINT ---\npkg.Svc/M\n' });
    } finally {
      globalThis.fetch = original;
    }
  });

  it('says why when the file cannot be read, rather than writing an empty one', async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: false, text: async () => 'gone' })) as any;
    try {
      await expect(useStore.getState().duplicateCollection('auth/login.gctf')).rejects.toThrow('could not be read');
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe('formatting the file on screen', () => {
  const withFetch = async (formatted: string, run: () => Promise<unknown>) => {
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string) => {
      if (String(url) === '/api/fmt') {
        return { ok: true, json: async () => ({ formatted, changed: true }) };
      }
      return { ok: true, json: async () => ({ content: 'a\nb\n', version: { mtime_ms: 1, hash: 'x' } }) };
    }) as any;
    try { return await run(); } finally { globalThis.fetch = original; }
  };

  it('writes the formatter\'s answer into the editor and says how much moved', async () => {
    useStore.setState({
      workspacePath: 'a.gctf', rawContent: 'a\nb\n', rawOriginal: 'a\nb\n',
      workspaceOriginal: null, collectionParsed: null,
    });
    const lines = await withFetch('a\nB\n', () => useStore.getState().formatFile());
    expect(lines).toBe(2);
    expect(useStore.getState().rawContent).toBe('a\nB\n');
  });

  it('answers nothing moved when nothing did', async () => {
    useStore.setState({ workspacePath: 'a.gctf', rawContent: 'a\nb\n', rawOriginal: 'a\nb\n' });
    const lines = await withFetch('a\nb\n', () => useStore.getState().formatFile());
    expect(lines).toBe(0);
  });

  it('refuses without a file', async () => {
    useStore.setState({ workspacePath: null });
    await expect(useStore.getState().formatFile()).rejects.toThrow('Open a file');
  });

  it('refuses while the forms are ahead of the file', async () => {
    const orig = makeParsed({ endpoint: 'a.A/One' });
    useStore.setState({
      workspacePath: 'a.gctf', rawContent: 'a\n', rawOriginal: 'a\n',
      workspaceOriginal: orig, collectionParsed: orig,
      request: { endpoint: 'a.A/Other', headers: orig.headers, bodies: orig.bodies },
    });
    await expect(useStore.getState().formatFile()).rejects.toThrow('save them first');
  });
});

describe('expecting an answer with nothing in it', () => {
  const armed = (messages: unknown[]) => {
    const orig = makeParsed({ endpoint: 'echo.Echo/Stream' });
    const tab = makeTab({ collectionParsed: orig, collectionOriginal: orig });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id,
      workspacePath: 'stream.gctf', collectionParsed: orig, workspaceOriginal: orig,
      documents: [], activeStep: 0,
      request: { endpoint: orig.endpoint, headers: orig.headers, bodies: orig.bodies },
      response: {
        status: 'ok', statusCode: 0, messages, headers: {}, trailers: {},
        error: null, durationMs: 4,
      } as any,
    });
  };

  it('writes the empty block a stream with no messages asks for', () => {
    armed([]);
    expect(useStore.getState().expectFromResponse()).toBe(true);
    const written = useStore.getState().collectionParsed!.expect_responses;
    expect(written).toHaveLength(1);
    expect(written[0].body).toBe('');
  });

  it('does not write it as an empty object', () => {
    armed([]);
    useStore.getState().expectFromResponse();
    expect(useStore.getState().collectionParsed!.expect_responses[0].body).not.toBe('{}');
  });

  it('still writes one block per message when there are some', () => {
    armed([{ a: 1 }, { a: 2 }]);
    useStore.getState().expectFromResponse();
    expect(useStore.getState().collectionParsed!.expect_responses.map(m => m.body))
      .toEqual(['{\n  "a": 1\n}', '{\n  "a": 2\n}']);
  });
});

describe('what a file has bound', () => {
  const armed = (over: Partial<ReturnType<typeof useStore.getState>>) => ({
    ...useStore.getState(),
    workspacePath: 'checkout.apif',
    ...over,
  });

  it('is nothing until something has answered', () => {
    expect(bindingsOf(armed({}))).toBeUndefined();
  });

  it('takes what a run bound', () => {
    const st = armed({
      run: { ...useStore.getState().run, verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok']] } } as any },
    });
    expect(bindingsOf(st)).toEqual([['who', 'ok']]);
  });

  it('and what a call bound, which wins over an older run', () => {
    const st = armed({
      run: { ...useStore.getState().run, verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok'], ['id', '1']] } } as any },
      executeBound: { 'checkout.apif': [['who', 'now']] },
    });
    expect(bindingsOf(st)).toEqual([['who', 'now'], ['id', '1']]);
  });

  it('answers with the same object while its sources are the same', () => {
    const st = armed({
      run: { ...useStore.getState().run, verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok']] } } as any },
      executeBound: { 'checkout.apif': [['id', '1']] },
    });
    expect(bindingsOf(st)).toBe(bindingsOf(st));
  });

  it('belongs to its own file', () => {
    const st = armed({ executeBound: { 'other.gctf': [['who', 'ok']] } });
    expect(bindingsOf(st)).toBeUndefined();
  });
});

describe('what the clipboard is told it holds', () => {
  function arm(over: Record<string, unknown>) {
    useStore.setState({
      workspacePath: 'checkout.apif',
      activeEnvironment: null,
      environments: [],
      request: { endpoint: 'GET /v1/users/{{who}}', headers: {}, bodies: [] },
      run: { ...useStore.getState().run, verdicts: {} },
      ...over,
    } as never);
  }

  it('says nothing when nothing was filled in', () => {
    arm({});
    expect(copyNote(useStore.getState())).toBe('');
  });

  it('names the run when its bindings filled something', () => {
    arm({ run: { ...useStore.getState().run, verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok']] } } } });
    expect(copyNote(useStore.getState())).toBe(' — the names this file has bound filled in');
  });

  it('stays quiet about a binding the request does not use', () => {
    arm({
      request: { endpoint: 'GET /v1/users', headers: {}, bodies: [] },
      run: { ...useStore.getState().run, verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok']] } } },
    });
    expect(copyNote(useStore.getState())).toBe('');
  });

  it('names both when both filled something', () => {
    arm({
      activeEnvironment: 'dev',
      environments: [{ name: 'dev', source: 'browser', variables: { HOST: 'x' } }],
      run: { ...useStore.getState().run, verdicts: { 'checkout.apif': { path: 'checkout.apif', state: 'pass', extracted: [['who', 'ok']] } } },
    });
    expect(copyNote(useStore.getState())).toBe(' — "dev" values and the names this file has bound filled in');
  });
});

describe('the address a draft was typed with', () => {
  it('travels with the tab it was typed for', () => {
    const draft = makeTab({ id: 'imported', collectionPath: null, collectionParsed: null, collectionOriginal: null });
    useStore.setState({
      tabs: [draft], activeTabId: draft.id, workspacePath: null,
      address: 'https://api.example.com', addressTouched: true,
    } as never);

    const kept = serializeTab({ ...draft, address: 'https://api.example.com', addressTouched: true });
    expect(kept.d).toBe('https://api.example.com');
    expect(deserializeTab(kept).address).toBe('https://api.example.com');
    expect(deserializeTab(kept).addressTouched).toBe(true);
  });

  it('comes back when the tab does', () => {
    const draft = makeTab({ id: 'imported', label: 'POST /v1/users', collectionPath: null, collectionParsed: null, collectionOriginal: null });
    const other = makeTab({ id: 'other', label: 'other.gctf', collectionPath: 'other.gctf' });
    useStore.setState({
      tabs: [draft, other], activeTabId: draft.id, workspacePath: null,
      address: '', addressTouched: false,
    } as never);
    useStore.getState().setAddress('https://api.example.com');

    useStore.getState().setActiveTab(other.id);
    expect(useStore.getState().addressTouched).toBe(false);

    useStore.getState().setActiveTab(draft.id);
    expect(useStore.getState().address).toBe('https://api.example.com');
    expect(useStore.getState().addressTouched).toBe(true);
  });

  it('keeps none for a tab that was never typed into', () => {
    const tab = makeTab({ id: 'plain', collectionPath: null });
    expect(serializeTab({ ...tab, addressTouched: false, address: 'localhost:4770' }).d).toBeUndefined();
  });

  it('keeps none for a tab with a file behind it', () => {
    const tab = makeTab({ id: 'saved', collectionPath: 'auth/login.gctf' });
    expect(serializeTab({ ...tab, addressTouched: true, address: 'https://api.example.com' }).d).toBeUndefined();
  });
});

describe('the tab an import lands in', () => {
  const call = { endpoint: 'POST /v1/users', headers: { 'content-type': 'application/json' }, bodies: ['{"name":"Ada"}'] };

  function arm(over: Partial<Tab> = {}) {
    const held = makeTab({
      id: 'held', label: 'POST /v1/users', collectionPath: null, collectionParsed: null, collectionOriginal: null,
      endpoint: call.endpoint, headers: call.headers, bodies: call.bodies, ...over,
    });
    const other = makeTab({ id: 'other', collectionPath: 'auth/login.gctf' });
    useStore.setState({ tabs: [held, other], activeTabId: other.id } as never);
    return held;
  }

  it('is the draft already holding that exact call', () => {
    const held = arm();
    expect(useStore.getState().focusHeldCall(call)).toBe(true);
    expect(useStore.getState().activeTabId).toBe(held.id);
  });

  it('pins the tab it lands in', () => {
    arm({ isPreview: true });
    useStore.getState().focusHeldCall(call);
    expect(useStore.getState().tabs.find(t => t.id === 'held')?.isPreview).toBe(false);
  });

  it('is a new tab when the body differs', () => {
    arm();
    expect(useStore.getState().focusHeldCall({ ...call, bodies: ['{"name":"Grace"}'] })).toBe(false);
  });

  it('is never a tab with a file behind it', () => {
    const saved = makeTab({ id: 'saved', collectionPath: 'auth/login.gctf', endpoint: call.endpoint, headers: call.headers, bodies: call.bodies });
    useStore.setState({ tabs: [saved], activeTabId: saved.id } as never);
    expect(useStore.getState().focusHeldCall(call)).toBe(false);
  });
});

describe('what the poll asks about', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => { globalThis.fetch = originalFetch; });

  it('asks only about the files that are open', async () => {
    const parsed = makeParsed({ endpoint: 'pkg.Svc/Old' });
    let asked: string[] = [];
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).startsWith('/api/versions')) {
        asked = JSON.parse(String(init?.body ?? '{"paths":[]}')).paths;
        return { ok: true, json: async () => Object.fromEntries(asked.map(p => [p, { mtime_ms: 100, hash: 'h100' }])) };
      }
      return { ok: true, json: async () => ({ parsed, documents: [], content: '', version: { mtime_ms: 100, hash: 'h100' } }) };
    }) as never;
    useStore.setState({ tabs: [], activeTabId: null, collections: [] } as never);
    await useStore.getState().loadCollection('one.gctf');
    await useStore.getState().loadCollection('two.gctf', { pin: true });

    await useStore.getState().syncOpenFiles();
    expect(new Set(asked)).toEqual(new Set(['one.gctf', 'two.gctf']));
  });

  it('reads the change out of the hash, not the clock', async () => {
    const before = makeParsed({ endpoint: 'pkg.Svc/Old' });
    const after = makeParsed({ endpoint: 'pkg.Svc/Restored' });
    let phase = 'before';
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).startsWith('/api/versions')) {
        const paths = JSON.parse(String(init?.body ?? '{"paths":[]}')).paths as string[];
        return { ok: true, json: async () => Object.fromEntries(paths.map(p => [p, { mtime_ms: 50, hash: 'h50' }])) };
      }
      const parsed = phase === 'before' ? before : after;
      const version = phase === 'before' ? { mtime_ms: 100, hash: 'h100' } : { mtime_ms: 50, hash: 'h50' };
      return { ok: true, json: async () => ({ parsed, documents: [], content: '', version }) };
    }) as never;
    useStore.setState({ tabs: [], activeTabId: null, collections: [] } as never);
    await useStore.getState().loadCollection('checked-out.gctf');
    phase = 'after';

    expect(await useStore.getState().syncOpenFiles()).toEqual(['checked-out.gctf']);
    expect(useStore.getState().collectionParsed?.endpoint).toBe('pkg.Svc/Restored');
  });
});

describe('a save the file refused', () => {
  const conflict = async () => {
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string) => {
      if (String(url).includes('/api/preview-structured')) {
        return { ok: true, json: async () => ({ content: 'mine' }) };
      }
      return {
        ok: false,
        status: 409,
        text: async () => JSON.stringify({ content: 'theirs', version: { mtime_ms: 2, hash: 'b' } }),
      };
    }) as never;
    try {
      await useStore.getState().saveWorkspace();
    } finally {
      globalThis.fetch = original;
    }
  };

  const armed = () => {
    const tab = makeTab({ collectionPath: 'a.gctf' });
    useStore.setState({
      tabs: [tab], activeTabId: tab.id, staleOnDisk: false,
      workspacePath: 'a.gctf',
      request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] },
      rawContent: null, rawOriginal: null,
    } as never);
  };

  it('marks the tab, so cancelling leaves the fact on screen', async () => {
    armed();
    await conflict();
    expect(useStore.getState().saveConflict).not.toBeNull();
    expect(useStore.getState().staleOnDisk).toBe(true);
    expect(useStore.getState().tabs[0].staleOnDisk).toBe(true);
  });

  it('keeps sending the version this tab was opened against', async () => {
    armed();
    const stamps: unknown[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).includes('/api/preview-structured')) {
        return { ok: true, json: async () => ({ content: 'mine' }) };
      }
      stamps.push(JSON.parse(String(init?.body ?? '{}')).version);
      return {
        ok: false,
        status: 409,
        text: async () => JSON.stringify({ content: 'theirs', version: { mtime_ms: 2, hash: 'theirs' } }),
      };
    }) as never;
    try {
      await useStore.getState().saveWorkspace();
      await useStore.getState().resolveSaveConflict('cancel');
      await useStore.getState().saveWorkspace();
    } finally {
      globalThis.fetch = original;
    }
    expect(stamps).toHaveLength(2);
    expect(stamps[1]).toEqual(stamps[0]);
    expect(useStore.getState().saveConflict).not.toBeNull();
  });

  it('takes the mark off when the disk version is taken', async () => {
    armed();
    await conflict();
    const original = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: false, status: 404, text: async () => '' })) as never;
    try {
      await useStore.getState().resolveSaveConflict('reload');
    } finally {
      globalThis.fetch = original;
    }
    expect(useStore.getState().staleOnDisk).toBe(false);
  });
});

describe('the command copied for an open file', () => {
  it('names the file, so the line can carry its schema', async () => {
    const sent: string[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = (async (url: string, init?: RequestInit) => {
      if (String(url).includes('/api/grpcurl')) {
        sent.push(String(init?.body ?? ''));
        return { ok: true, json: async () => ({ command: 'grpcurl …' }) };
      }
      return { ok: true, json: async () => ({}), text: async () => '' };
    }) as never;
    try {
      useStore.setState({
        workspacePath: 'auth/login.gctf',
        request: { endpoint: 'a.B/C', headers: {}, bodies: ['{"a":1}'] },
      } as never);
      await useStore.getState().getGrpcurlCommand();
    } finally {
      globalThis.fetch = original;
    }
    expect(sent).toHaveLength(1);
    expect(JSON.parse(sent[0]).collection_path).toBe('auth/login.gctf');
  });
});
