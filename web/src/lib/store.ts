import { create } from 'zustand';
import type { PlayStore, HistoryEntry, CallResult, CollectionParsed, Tab, StoredTab, TabsStorage, Environment, WireProtocol } from './types';
import { ENVS_KEY, ACTIVE_ENV_KEY, TABS_KEY, SETTINGS_KEY, defaultAddressFor } from './types';
import type { ClientSettings } from './types';
import { LRUCache } from './cache';
import { getSessionId } from './session';
import { applyEnvironment, substituteEnv } from './env';

function now() { return Date.now(); }
function id() { return Math.random().toString(36).slice(2, 9); }

const DEFAULT_BODY = '{}';
const DEFAULT_BODIES = [DEFAULT_BODY];
const historyCache = new LRUCache<string, HistoryEntry>(1000);
// Per-tab AbortControllers so executing in one tab never cancels another.
const abortControllers = new Map<string, AbortController>();
let reflectController: AbortController | null = null;

const EMPTY_REQUEST = { endpoint: '', headers: {}, bodies: DEFAULT_BODIES };


async function initProjectEnvs(envNames: string[]) {
  if (envNames.length === 0) return;
  const projectEnvs: Environment[] = [];
  for (const name of envNames) {
    try {
      const res = await fetch(`/api/project/env/${encodeURIComponent(name)}/merged`);
      if (!res.ok) continue;
      const data = await res.json();
      projectEnvs.push({ name, address: data.address || undefined, variables: data.variables });
    } catch {  }
  }
  if (projectEnvs.length > 0) {
    useStore.setState(s => {
      const existing = s.environments.filter(e => !projectEnvs.some(p => p.name === e.name));
      return { environments: [...projectEnvs, ...existing] };
    });
  }
}

const STORAGE_KEY = 'grpctestify-history';
const TOTALS_KEY = 'grpctestify-totals';
const MAX_STORAGE_BYTES = 4_000_000;
const MAX_TABS = 50;



function defaultTab(): Tab {
  const tId = id();
  return {
    id: tId,
    label: 'Untitled',
    endpoint: '',
    headers: {},
    bodies: [...DEFAULT_BODIES],
    environment: {},
    response: null,
    requestTab: 'body',
    gctfTab: 'request',
    responseTab: 'response',
    collectionPath: null,
    collectionParsed: null,
    collectionOriginal: null,
  };
}


function snapshot(state: PlayStore, tabId: string, overrides?: Partial<Tab>): Tab {
  const existing = state.tabs.find(t => t.id === tabId);
  return {
    ...(existing || defaultTab()),
    id: tabId,
    label: existing?.label || 'Untitled',
    endpoint: overrides?.endpoint ?? state.request.endpoint,
    headers: overrides?.headers ?? state.request.headers,
    bodies: overrides?.bodies ?? state.request.bodies,
    environment: overrides?.environment ?? state.environment,
    response: overrides?.response ?? state.response,
    requestTab: overrides?.requestTab ?? state.requestTab,
    gctfTab: overrides?.gctfTab ?? state.gctfTab,
    responseTab: overrides?.responseTab ?? state.responseTab,
    collectionPath: overrides?.collectionPath ?? state.workspacePath,
    collectionParsed: overrides?.collectionParsed ?? state.collectionParsed,
    collectionOriginal: overrides?.collectionOriginal ?? state.workspaceOriginal,
    ...overrides,
  };
}


function loadTab(tab: Tab) {
  return {
    request: { endpoint: tab.endpoint, headers: tab.headers, bodies: tab.bodies },
    environment: tab.environment || {},
    response: tab.response,
    requestTab: tab.requestTab,
    gctfTab: tab.gctfTab,
    responseTab: tab.responseTab,
    workspacePath: tab.collectionPath,
    collectionParsed: tab.collectionParsed,
    collectionOriginal: tab.collectionOriginal,
    selectedCollection: tab.collectionPath,
  };
}



function serializeTab(t: Tab): StoredTab {
  return {
    i: t.id, l: t.label, e: t.endpoint, h: t.headers, b: t.bodies, c: t.collectionPath,
    v: t.environment && Object.keys(t.environment).length > 0 ? t.environment : undefined,
  };
}

function deserializeTab(s: StoredTab): Tab {
  const tId = s.i || id();
  return {
    id: tId,
    label: s.l || 'Untitled',
    endpoint: s.e || '',
    headers: s.h || {},
    bodies: (s.b && s.b.length > 0) ? s.b : [...DEFAULT_BODIES],
    environment: s.v || {},
    response: null,
    requestTab: 'body',
    gctfTab: 'request',
    responseTab: 'response',
    collectionPath: s.c || null,
    collectionParsed: null,
    collectionOriginal: null,
  };
}

function saveTabsToStorage(tabs: Tab[], activeTabId: string | null) {
  try {
    const stored: TabsStorage = {
      t: tabs.map(serializeTab),
      a: activeTabId,
    };
    localStorage.setItem(TABS_KEY, JSON.stringify(stored));
  } catch {  }
}

function loadTabsFromStorage(): { tabs: Tab[]; activeTabId: string | null } {
  try {
    const raw = localStorage.getItem(TABS_KEY);
    if (!raw) return { tabs: [defaultTab()], activeTabId: null };
    const stored: TabsStorage = JSON.parse(raw);
    if (!stored.t || !Array.isArray(stored.t) || stored.t.length === 0) return { tabs: [defaultTab()], activeTabId: null };
    const tabs = stored.t.map(deserializeTab);
    const activeTabId = stored.a && tabs.some(t => t.id === stored.a) ? stored.a : tabs[0].id;
    return { tabs, activeTabId };
  } catch {
    return { tabs: [defaultTab()], activeTabId: null };
  }
}



function loadHistoryFromStorage(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const entries: HistoryEntry[] = JSON.parse(raw);
    if (!Array.isArray(entries)) return [];
    historyCache.clear();
    for (const e of entries) {
      if (e && e.id) historyCache.put(e.id, e);
    }
    return historyCache.values();
  } catch {
    try { localStorage.removeItem(STORAGE_KEY); } catch {  }
    return [];
  }
}

function saveHistoryToStorage() {
  try {
    const entries = historyCache.values();
    const json = JSON.stringify(entries);
    if (json.length <= MAX_STORAGE_BYTES) {
      localStorage.setItem(STORAGE_KEY, json);
    } else {
      const items = entries.slice();
      while (items.length > 0) {
        items.pop();
        const trimmed = JSON.stringify(items);
        if (trimmed.length <= MAX_STORAGE_BYTES || items.length <= 1) {
          localStorage.setItem(STORAGE_KEY, trimmed);
          historyCache.clear();
          for (const e of items) historyCache.put(e.id, e);
          break;
        }
      }
    }
  } catch {  }
}



const DEFAULT_SETTINGS: ClientSettings = {
  address: 'localhost:4770',
  protocol: 'grpc',
  tls: false,
  tlsInsecure: true,
  requestTimeoutMs: 0,
};

function loadSettings(): ClientSettings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw);
    return { ...DEFAULT_SETTINGS, ...parsed };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(s: ClientSettings) {
  try { localStorage.setItem(SETTINGS_KEY, JSON.stringify(s)); } catch {  }
}

// Extract only the small ClientSettings fields — never serialize tabs/history/
// responses/collections into the settings key.
function clientSettings(s: PlayStore): ClientSettings {
  return {
    address: s.address,
    protocol: s.protocol,
    tls: s.tls,
    tlsInsecure: s.tlsInsecure,
    requestTimeoutMs: s.requestTimeoutMs,
  };
}

function saveTotals(ok: number, error: number) {
  try { localStorage.setItem(TOTALS_KEY, JSON.stringify({ ok, error })); } catch {  }
}


const initTabs = loadTabsFromStorage();
const initActive = initTabs.activeTabId || initTabs.tabs[0]?.id || null;
const initTab = initTabs.tabs.find(t => t.id === initActive) || initTabs.tabs[0];


const initialSettings = loadSettings();

export const useStore = create<PlayStore>((set, get) => ({
  address: initialSettings.address,
  protocol: initialSettings.protocol,
  tls: initialSettings.tls,
  tlsInsecure: initialSettings.tlsInsecure,
  requestTimeoutMs: initialSettings.requestTimeoutMs,
  environment: initTab?.environment || {},
  collections: [],
  projectRoot: null,
  projectEnvNames: [],

  tabs: initTabs.tabs,
  activeTabId: initActive,

  workspacePath: initTab?.collectionPath || null,
  workspaceOriginal: null,
  selectedCollection: initTab?.collectionPath || null,
  collectionParsed: null,
  request: initTab ? { endpoint: initTab.endpoint, headers: initTab.headers, bodies: initTab.bodies } : { ...EMPTY_REQUEST },
  requestTab: 'body',
  gctfTab: 'request',
  response: null,
  responseTab: 'response',

  history: [],
  totalOk: (() => { try { return JSON.parse(localStorage.getItem(TOTALS_KEY) || '{}').ok || 0; } catch { return 0; } })(),
  totalError: (() => { try { return JSON.parse(localStorage.getItem(TOTALS_KEY) || '{}').error || 0; } catch { return 0; } })(),
  version: '',
  sessionId: getSessionId(),
  theme: (() => {
    let t: 'light' | 'dark' = 'light';
    try {
      const saved = localStorage.getItem('grpctestify-theme');
      if (saved === 'dark' || saved === 'light') t = saved;
      else t = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    } catch {  }
    document.documentElement.setAttribute('data-theme', t);
    return t;
  })(),
  reflectionMethods: [],
  reflectStatus: 'idle',
  reflectError: null,
  serverHealthy: true,
  collectionsMtime: 0,
  sidebarVisible: true,
  showHotkeyHelp: false,
  environments: (() => {
    try { return JSON.parse(localStorage.getItem(ENVS_KEY) || '[]'); }
    catch { return []; }
  })(),
  activeEnvironment: (() => {
    try { return localStorage.getItem(ACTIVE_ENV_KEY); }
    catch { return null; }
  })(),

  

  setAddress: (v) => { set({ address: v }); saveSettings(clientSettings(get())); },
  setProtocol: (v) => {
    const s = get();
    const updates: Partial<PlayStore> = { protocol: v };
    if (s.address === defaultAddressFor(s.protocol as WireProtocol)) {
      updates.address = defaultAddressFor(v);
    }
    set(updates);
    saveSettings(clientSettings(get()));
  },
  setTls: (v) => { set({ tls: v }); saveSettings(clientSettings(get())); },
  setTlsInsecure: (v) => { set({ tlsInsecure: v }); saveSettings(clientSettings(get())); },
  setRequestTimeoutMs: (v) => { set({ requestTimeoutMs: v }); saveSettings(clientSettings(get())); },

  setEndpoint: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, endpoint: v } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, endpoint: v } };
  }),

  setRequestBody: (idx, v) => set(s => {
    const bodies = s.request.bodies.map((b, i) => i === idx ? v : b);
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, bodies } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, bodies } };
  }),

  addRequestBody: () => set(s => {
    const bodies = [...s.request.bodies, DEFAULT_BODY];
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, bodies } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, bodies } };
  }),

  removeRequestBody: (idx) => set(s => {
    const bodies = s.request.bodies.length > 1 ? s.request.bodies.filter((_, i) => i !== idx) : s.request.bodies;
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, bodies } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, bodies } };
  }),

  setRequestBodies: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, bodies: v } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, bodies: v } };
  }),

  setRequestHeaders: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, headers: v } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, headers: v } };
  }),

  setRequestTab: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, requestTab: v } : t);
    return { tabs, requestTab: v };
  }),

  setGctfTab: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, gctfTab: v } : t);
    return { tabs, gctfTab: v };
  }),

  setResponseTab: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, responseTab: v } : t);
    return { tabs, responseTab: v };
  }),

  setCollections: (v) => set({ collections: v }),
  setCollectionParsed: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, collectionParsed: v } : t);
    return { tabs, collectionParsed: v };
  }),

  setEnvironment: (v: Record<string, string>) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, environment: v } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, environment: v };
  }),

  setTheme: (v) => {
    document.documentElement.setAttribute('data-theme', v);
    try { localStorage.setItem('grpctestify-theme', v); } catch {  }
    set({ theme: v });
  },

  setReflectionMethods: (v) => set({ reflectionMethods: v, reflectStatus: v.length > 0 ? 'ok' : 'error' }),

  reflect: async () => {
    const { address, protocol, tls, tlsInsecure, workspacePath } = get();
    if (!address) return;
    set({ reflectStatus: 'loading', reflectError: null });
    if (reflectController) reflectController.abort();
    reflectController = new AbortController();
    const reflector = reflectController;
    const timeoutId = setTimeout(() => reflector.abort(), 30_000);
    try {
      const res = await fetch('/api/reflect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          address,
          tls: tls || undefined,
          tls_insecure: tls ? tlsInsecure : undefined,
          collection_path: workspacePath || undefined,
          protocol: protocol || undefined,
        }),
        signal: reflector.signal,
      });
      if (reflectController === reflector) reflectController = null;
      const data = await res.json();
      if (data.error) {
        set({ reflectionMethods: [], reflectStatus: 'error', reflectError: data.error });
        return;
      }
      const services: any[] = data.services || [];
      const methods = services.flatMap((s: any) => (s.methods || []).map((m: any) => ({
        name: m.name,
        fullName: m.full_name,
        service: s.name,
      })));
      set({ reflectionMethods: methods, reflectStatus: methods.length > 0 ? 'ok' : 'error', reflectError: methods.length === 0 ? 'No methods found' : null });
    } catch {
      if (reflectController === reflector) reflectController = null;
      set({ reflectionMethods: [], reflectStatus: 'error', reflectError: 'Network error' });
    } finally {
      clearTimeout(timeoutId);
    }
  },

  cancel: () => {
    const st = get();
    const tabId = st.activeTabId;
    let aborted = false;
    if (tabId) {
      const c = abortControllers.get(tabId);
      if (c) { c.abort(); abortControllers.delete(tabId); aborted = true; }
    }
    if (reflectController) { reflectController.abort(); reflectController = null; aborted = true; }
    if (!aborted) return;
    set(s => {
      const tabs = s.tabs.map(t => t.id === tabId ? { ...t, response: null } : t);
      return { tabs, response: null };
    });
  },

  

  addTab: (config?) => {
    const newTab = { ...defaultTab(), ...config, id: id(), response: null };
    const state = get();
    if (state.tabs.length >= MAX_TABS) return state.activeTabId || newTab.id;
    const tabs = [...state.tabs, newTab];
    saveTabsToStorage(tabs, newTab.id);
    set({
      tabs,
      activeTabId: newTab.id,
      ...loadTab(newTab),
    });
    return newTab.id;
  },

  removeTab: (tabId) => {
    const state = get();
    if (state.tabs.length <= 1) return; 
    const idx = state.tabs.findIndex(t => t.id === tabId);
    if (idx === -1) return;
    const tabs = state.tabs.filter(t => t.id !== tabId);
    let activeTabId = state.activeTabId;
    if (activeTabId === tabId) {
      
      const neighbor = tabs[Math.min(idx, tabs.length - 1)];
      activeTabId = neighbor?.id || tabs[0]?.id || null;
    }
    saveTabsToStorage(tabs, activeTabId);
    const activeTab = tabs.find(t => t.id === activeTabId) || tabs[0];
    set({ tabs, activeTabId, ...loadTab(activeTab) });
  },

  setActiveTab: (tabId) => {
    const state = get();
    if (tabId === state.activeTabId) return;
    
    const snap = snapshot(state, state.activeTabId!);
    const tabs = state.tabs.map(t => t.id === state.activeTabId ? snap : t);
    
    const newTab = tabs.find(t => t.id === tabId);
    if (!newTab) return;
    saveTabsToStorage(tabs, tabId);
    set({
      tabs,
      activeTabId: tabId,
      ...loadTab(newTab),
    });
  },

  getTabLabel: (tabId) => {
    const tab = get().tabs.find(t => t.id === tabId);
    return tab?.label || '';
  },

  setTabLabel: (tabId, label) => {
    set(s => {
      const tabs = s.tabs.map(t => t.id === tabId ? { ...t, label } : t);
      saveTabsToStorage(tabs, s.activeTabId);
      return { tabs };
    });
  },

  

  newWorkspace: () => {
    const newTab = defaultTab();
    const state = get();
    const tabs = [...state.tabs, newTab];
    saveTabsToStorage(tabs, newTab.id);
    set({
      tabs,
      activeTabId: newTab.id,
      ...loadTab(newTab),
    });
  },

  loadCollection: async (path: string) => {
    const state = get();
    
    const existing = state.tabs.find(t => t.collectionPath === path);
    if (existing) {
      get().setActiveTab(existing.id);
      return;
    }
    
    const res = await fetch(`/api/collections/${path}`);
    if (!res.ok) return;
    const data = await res.json();
    const p: CollectionParsed = data.parsed;
    const label = path.split('/').pop() || path;
    const newTab: Tab = {
      ...defaultTab(),
      id: id(),
      label,
      endpoint: p.endpoint,
      headers: p.headers,
      bodies: p.bodies.length > 0 ? p.bodies : [...DEFAULT_BODIES],
      collectionPath: path,
      collectionParsed: p,
      collectionOriginal: p,
      requestTab: 'body',
      gctfTab: 'request',
      responseTab: 'response',
      response: null,
    };
    const tabs = [...state.tabs, newTab];
    saveTabsToStorage(tabs, newTab.id);
    set({ tabs, activeTabId: newTab.id, ...loadTab(newTab) });
  },

  saveWorkspace: async () => {
    const st = get();
    if (!st.workspacePath) return;
    const finalName = st.workspacePath.endsWith('.gctf') ? st.workspacePath : `${st.workspacePath}.gctf`;
    const res = await fetch('/api/save-structured', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path: finalName,
        endpoint: st.request.endpoint,
        bodies: st.request.bodies,
        headers: Object.keys(st.request.headers).length > 0 ? st.request.headers : undefined,
        address: st.address || undefined,
        original_path: st.workspacePath,
      }),
    });
    if (!res.ok) {
      const text = await res.text().catch(() => 'Save failed');
      throw new Error(text);
    }
    const updatedParsed = { ...st.collectionParsed! };
    updatedParsed.endpoint = st.request.endpoint;
    updatedParsed.headers = st.request.headers;
    updatedParsed.bodies = st.request.bodies;
    set(s => {
      const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, collectionOriginal: updatedParsed as any, collectionParsed: updatedParsed as any } : t);
      return { tabs, workspaceOriginal: updatedParsed as any, collectionParsed: updatedParsed as any };
    });
    get().refreshCollections();
  },

  saveWorkspaceAs: async (name: string) => {
    const st = get();
    const finalName = name.endsWith('.gctf') ? name : `${name}.gctf`;
    const res = await fetch('/api/save-structured', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        path: finalName,
        endpoint: st.request.endpoint,
        bodies: st.request.bodies,
        headers: Object.keys(st.request.headers).length > 0 ? st.request.headers : undefined,
        address: st.address || undefined,
      }),
    });
    if (!res.ok) {
      const text = await res.text().catch(() => 'Save failed');
      throw new Error(text);
    }
    const label = finalName.split('/').pop() || finalName;
    set(s => {
      const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, collectionPath: finalName, label } : t);
      saveTabsToStorage(tabs, s.activeTabId);
      return { tabs, workspacePath: finalName, selectedCollection: finalName };
    });
    get().refreshCollections();
  },

  getGrpcurlCommand: async () => {
    const { request, address, tls, tlsInsecure, protocol } = get();
    // Keep each body as its raw JSON literal so int64 precision survives (a
    // JS JSON.parse round-trip would corrupt integers > 2^53). Non-JSON bodies
    // fall back to a quoted string, preserving the old lenient behaviour.
    const encoded = request.bodies
      .map(b => b.trim())
      .filter(b => b && b !== '')
      .map(b => { try { JSON.parse(b); return b; } catch { return JSON.stringify(b); } });
    const bodyLiteral = encoded.length === 0
      ? '{}'
      : encoded.length === 1 ? encoded[0] : `[${encoded.join(',')}]`;
    const meta: Record<string, unknown> = {
      endpoint: request.endpoint,
      headers: Object.keys(request.headers).length > 0 ? request.headers : undefined,
      address: address || undefined,
      tls: tls || undefined,
      tls_insecure: tls ? tlsInsecure : undefined,
      protocol: protocol || undefined,
    };
    // Build the payload manually to splice in the raw body literal unmodified.
    const metaJson = JSON.stringify(meta);
    const payload = metaJson === '{}'
      ? `{"body":${bodyLiteral}}`
      : `${metaJson.slice(0, -1)},"body":${bodyLiteral}}`;
    const res = await fetch('/api/grpcurl', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: payload,
    });
    if (!res.ok) throw new Error('Failed to generate grpcurl command');
    const data = await res.json();
    if (data.command?.startsWith('# error')) throw new Error(data.command);
    return data.command || 'grpcurl ...';
  },

  execute: async () => {
    const st = get();
    const { workspacePath, tls, tlsInsecure, address, protocol } = st;

    // Capture the tab that issued this request. All response writes below target
    // THIS tab, not whichever tab happens to be active when the call completes.
    const tabId = st.activeTabId;

    // Write a response into the issuing tab; only mirror it into the global
    // `response` field when the issuing tab is still the active one.
    const writeResponse = (r: CallResult | null, extra?: Partial<PlayStore>) => set(s => {
      const tabs = s.tabs.map(t => t.id === tabId ? { ...t, response: r } : t);
      const patch: Partial<PlayStore> = { tabs, ...extra };
      if (s.activeTabId === tabId) patch.response = r;
      return patch;
    });

    const activeEnv = st.activeEnvironment
      ? st.environments.find(e => e.name === st.activeEnvironment)
      : null;

    if (!st.request.endpoint) {
      const errResult: CallResult = { status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: 'Enter a gRPC endpoint', durationMs: null };
      writeResponse(errResult);
      return;
    }


    const effectiveEnv = activeEnv ? {
      ...activeEnv,
      variables: Object.fromEntries(
        Object.entries(activeEnv.variables)
          .filter(([k]) => !(activeEnv.mutedVariables || []).includes(k))
      ),
    } : null;

    const substituted = applyEnvironment(st.request.endpoint, st.request.headers, st.request.bodies, effectiveEnv);

    const effectiveAddress = activeEnv
      ? substituteEnv(address, effectiveEnv) || address
      : address;

    // Per-tab AbortController: aborting one tab must not cancel another.
    if (tabId) {
      const prev = abortControllers.get(tabId);
      if (prev) prev.abort();
    }
    const controller = new AbortController();
    if (tabId) abortControllers.set(tabId, controller);
    const clearController = () => { if (tabId && abortControllers.get(tabId) === controller) abortControllers.delete(tabId); };
    const signal = controller.signal;
    let timeoutId: number | undefined;
    if (st.requestTimeoutMs > 0) {
      timeoutId = window.setTimeout(() => controller.abort(), st.requestTimeoutMs);
    }

    const pending: CallResult = { status: 'pending', statusCode: null, messages: [], headers: {}, trailers: {}, error: null, durationMs: null };
    writeResponse(pending);

    const start = performance.now();
    try {
      const filteredBodies = substituted.bodies.filter(b => b.trim() && b !== '');
      const bodies_raw = filteredBodies.length > 0 ? filteredBodies : undefined;

      const res = await fetch('/api/call', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          endpoint: substituted.endpoint,
          bodies_raw,
          headers: Object.keys(substituted.headers).length > 0 ? substituted.headers : undefined,
          tls: tls || undefined, tls_insecure: tls ? tlsInsecure : undefined,
          address: effectiveAddress || undefined,
          protocol: protocol || undefined,
          environment: Object.keys(st.environment).length > 0 ? st.environment : undefined,
          collection_path: workspacePath || undefined,
          session_id: st.sessionId || undefined,
        }),
        signal,
      });
      clearController();

      let data: any;
      try { data = await res.json(); } catch {
        const errResult: CallResult = { status: 'error', statusCode: res.status, messages: [], headers: {}, trailers: {}, error: `Server returned ${res.status} ${res.statusText}`, durationMs: Math.round(performance.now() - start) };
        const errEntry: HistoryEntry = { id: id(), timestamp: now(), endpoint: st.request.endpoint, bodies: st.request.bodies, headers: st.request.headers, response: errResult };
        historyCache.put(errEntry.id, errEntry);
        saveHistoryToStorage();
        const totalError = get().totalError + 1;
        saveTotals(get().totalOk, totalError);
        writeResponse(errResult, { history: historyCache.values(), totalError });
        return;
      }

      const durationMs = Math.round(performance.now() - start);
      const result: CallResult = {
        status: data.success ? 'ok' : 'error',
        statusCode: res.status,
        messages: data.messages ?? [],
        headers: data.headers || {},
        trailers: data.trailers || {},
        error: data.error || null,
        durationMs,
      };
      const entry: HistoryEntry = { id: id(), timestamp: now(), endpoint: st.request.endpoint, bodies: st.request.bodies, headers: st.request.headers, response: result };
      historyCache.put(entry.id, entry);
      saveHistoryToStorage();
      if (data.success) {
        const totalOk = get().totalOk + 1;
        saveTotals(totalOk, get().totalError);
        writeResponse(result, { history: historyCache.values(), totalOk });
      } else {
        const totalError = get().totalError + 1;
        saveTotals(get().totalOk, totalError);
        writeResponse(result, { history: historyCache.values(), totalError });
      }
    } catch (err: any) {
      clearController();
      if (err?.name === 'AbortError') {
        writeResponse(null);
        return;
      }
      const errResult: CallResult = { status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: err?.message || String(err), durationMs: Math.round(performance.now() - start) };
      const errEntry: HistoryEntry = { id: id(), timestamp: now(), endpoint: st.request.endpoint, bodies: st.request.bodies, headers: st.request.headers, response: errResult };
      historyCache.put(errEntry.id, errEntry);
      saveHistoryToStorage();
      const totalError = get().totalError + 1;
      saveTotals(get().totalOk, totalError);
      writeResponse(errResult, { history: historyCache.values(), totalError });
    } finally {
      if (timeoutId !== undefined) clearTimeout(timeoutId);
    }
  },

  loadStartupInfo: async () => {
    try {
      const res = await fetch('/api/info');
      if (!res.ok) { set({ serverHealthy: false }); return; }
      const data = await res.json();
      set({
        version: data.version || '',
        serverHealthy: data.status === 'ok',
        collectionsMtime: data.collections_mtime ?? 0,
      });
      
      if (data.project?.active) {
        set({ projectRoot: data.project.project_dir || '.grpctestify', projectEnvNames: data.project.envs || [] });
        await initProjectEnvs(data.project.envs || []);
        const sdata = await fetch('/api/project/settings').then(r => r.ok ? r.json() : null).catch(() => null);
        if (sdata) {
          set({
            address: sdata.address || get().address,
            protocol: sdata.protocol || get().protocol,
            tls: sdata.tls ?? get().tls,
            tlsInsecure: sdata.tls_insecure ?? get().tlsInsecure,
          });
          if (sdata.active_env && get().environments.some(e => e.name === sdata.active_env)) {
            set({ activeEnvironment: sdata.active_env });
          }
        }
      }
    } catch { set({ serverHealthy: false }); }
  },

  checkHealth: async () => {
    try {
      const res = await fetch('/api/health');
      set({ serverHealthy: res.ok });
    } catch { set({ serverHealthy: false }); }
  },

  setActiveEnvironment: (name) => {
    try {
      if (name) localStorage.setItem(ACTIVE_ENV_KEY, name);
      else localStorage.removeItem(ACTIVE_ENV_KEY);
    } catch {  }
    set({ activeEnvironment: name });
  },
  addEnvironment: (env) => {
    set(s => {
      const envs = [...s.environments.filter(e => e.name !== env.name), env];
      try { localStorage.setItem(ENVS_KEY, JSON.stringify(envs)); } catch {  }
      return { environments: envs };
    });
  },
  updateEnvironment: (name, env) => {
    set(s => {
      const envs = s.environments.map(e => e.name === name ? env : e);
      try { localStorage.setItem(ENVS_KEY, JSON.stringify(envs)); } catch {  }
      return { environments: envs };
    });
  },
  deleteEnvironment: (name) => {
    set(s => {
      const envs = s.environments.filter(e => e.name !== name);
      try { localStorage.setItem(ENVS_KEY, JSON.stringify(envs)); } catch {  }
      return {
        environments: envs,
        activeEnvironment: s.activeEnvironment === name ? null : s.activeEnvironment,
      };
    });
  },

  muteVariable: (envName, key) => {
    set(s => {
      const envs = s.environments.map(e => {
        if (e.name !== envName) return e;
        const muted = e.mutedVariables ? [...e.mutedVariables] : [];
        if (!muted.includes(key)) muted.push(key);
        const updated = { ...e, mutedVariables: muted };
        return updated;
      });
      try { localStorage.setItem(ENVS_KEY, JSON.stringify(envs)); } catch {  }
      return { environments: envs };
    });
  },
  unmuteVariable: (envName, key) => {
    set(s => {
      const envs = s.environments.map(e => {
        if (e.name !== envName) return e;
        const muted = (e.mutedVariables || []).filter(k => k !== key);
        return { ...e, mutedVariables: muted.length > 0 ? muted : undefined };
      });
      try { localStorage.setItem(ENVS_KEY, JSON.stringify(envs)); } catch {  }
      return { environments: envs };
    });
  },

  restoreHistory: (entry) => {
    const state = get();
    const newTab: Tab = {
      ...defaultTab(),
      id: id(),
      label: entry.endpoint || 'History',
      endpoint: entry.endpoint,
      headers: entry.headers,
      bodies: entry.bodies,
      response: entry.response,
    };
    const tabs = [...state.tabs, newTab];
    saveTabsToStorage(tabs, newTab.id);
    set({ tabs, activeTabId: newTab.id, ...loadTab(newTab) });
  },

  setHistory: (v) => set({ history: v }),
  clearHistory: () => {
    historyCache.clear();
    try { localStorage.removeItem(STORAGE_KEY); } catch {  }
    set({ history: [] });
  },

  

  saveProjectSettings: async (s) => {
    try {
      await fetch('/api/project/settings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(s),
      });
    } catch {  }
  },

  fetchProjectEnv: async (name) => {
    const res = await fetch(`/api/project/env/${encodeURIComponent(name)}`);
    if (!res.ok) throw new Error(`Failed to fetch env: ${res.statusText}`);
    return res.json();
  },

  saveProjectEnv: async (name, content) => {
    const res = await fetch(`/api/project/env/${encodeURIComponent(name)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content }),
    });
    if (!res.ok) throw new Error(`Failed to save env: ${res.statusText}`);
  },

  fetchProjectEnvLocal: async (name) => {
    const res = await fetch(`/api/project/env/${encodeURIComponent(name)}/local`);
    if (!res.ok) return { exists: false, content: null };
    return res.json();
  },

  saveProjectEnvLocal: async (name, content) => {
    const res = await fetch(`/api/project/env/${encodeURIComponent(name)}/local`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content }),
    });
    if (!res.ok) throw new Error(`Failed to save local env: ${res.statusText}`);
  },

  deleteProjectEnvLocal: async (name) => {
    await fetch(`/api/project/env/${encodeURIComponent(name)}/local`, { method: 'DELETE' });
  },

  toggleSidebar: () => set(s => ({ sidebarVisible: !s.sidebarVisible })),
  setShowHotkeyHelp: (v) => set({ showHotkeyHelp: v }),

  refreshCollections: async () => {
    try { const res = await fetch('/api/collections'); if (res.ok) set({ collections: await res.json() }); } catch {  }
  },
}));


const stored = loadHistoryFromStorage();
if (stored.length > 0) {
  useStore.getState().setHistory(stored);
}



