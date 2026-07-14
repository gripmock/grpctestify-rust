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
let abortController: AbortController | null = null;
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
  return { i: t.id, l: t.label, e: t.endpoint, h: t.headers, b: t.bodies, c: t.collectionPath };
}

function deserializeTab(s: StoredTab): Tab {
  const tId = s.i || id();
  return {
    id: tId,
    label: s.l || 'Untitled',
    endpoint: s.e || '',
    headers: s.h || {},
    bodies: (s.b && s.b.length > 0) ? s.b : [...DEFAULT_BODIES],
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
  environment: {},
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

  

  setAddress: (v) => { set({ address: v }); saveSettings({ ...get(), address: v }); },
  setProtocol: (v) => {
    const s = get();
    const updates: Partial<PlayStore> = { protocol: v };
    if (s.address === defaultAddressFor(s.protocol as WireProtocol)) {
      updates.address = defaultAddressFor(v);
    }
    set(updates);
    saveSettings({ ...s, ...updates });
  },
  setTls: (v) => { set({ tls: v }); saveSettings({ ...get(), tls: v }); },
  setTlsInsecure: (v) => { set({ tlsInsecure: v }); saveSettings({ ...get(), tlsInsecure: v }); },
  setRequestTimeoutMs: (v) => { set({ requestTimeoutMs: v }); saveSettings({ ...get(), requestTimeoutMs: v }); },

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

  setEnvironment: (v: Record<string, string>) => set({ environment: v }),

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

  isDirty: () => {
    const { workspaceOriginal, request } = get();
    if (!workspaceOriginal) return request.endpoint !== '' || request.bodies.some(b => b !== DEFAULT_BODY);
    return (
      request.endpoint !== workspaceOriginal.endpoint ||
      JSON.stringify(request.headers) !== JSON.stringify(workspaceOriginal.headers) ||
      JSON.stringify(request.bodies) !== JSON.stringify(workspaceOriginal.bodies)
    );
  },

  cancel: () => {
    let aborted = false;
    if (abortController) { abortController.abort(); abortController = null; aborted = true; }
    if (reflectController) { reflectController.abort(); reflectController = null; aborted = true; }
    if (!aborted) return;
    set(s => {
      const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, response: null } : t);
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
    const { request } = get();
    const parsed: unknown[] = request.bodies
      .map(b => { try { return JSON.parse(b); } catch { return b; } })
      .filter(b => b !== '');
    const body = parsed.length === 1 ? parsed[0] : parsed;
    const res = await fetch('/api/grpcurl', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ endpoint: request.endpoint, body, headers: Object.keys(request.headers).length > 0 ? request.headers : undefined }),
    });
    if (!res.ok) throw new Error('Failed to generate grpcurl command');
    const data = await res.json();
    if (data.command?.startsWith('# error')) throw new Error(data.command);
    return data.command || 'grpcurl ...';
  },

  execute: async () => {
    const st = get();
    const { workspacePath, tls, tlsInsecure, address, protocol } = st;

    const activeEnv = st.activeEnvironment
      ? st.environments.find(e => e.name === st.activeEnvironment)
      : null;

    if (!st.request.endpoint) {
      set(s => {
        const errResult: CallResult = { status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: 'Enter a gRPC endpoint', durationMs: null };
        const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, response: errResult } : t);
        return { tabs, response: errResult };
      });
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

    if (abortController) abortController.abort();
    const controller = new AbortController();
    abortController = controller;
    const signal = controller.signal;
    let timeoutId: number | undefined;
    if (st.requestTimeoutMs > 0) {
      timeoutId = window.setTimeout(() => controller.abort(), st.requestTimeoutMs);
    }

    const pending: CallResult = { status: 'pending', statusCode: null, messages: [], headers: {}, trailers: {}, error: null, durationMs: null };
    set(s => {
      const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, response: pending } : t);
      return { tabs, response: pending };
    });

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
          collection_path: workspacePath || undefined,
          session_id: st.sessionId || undefined,
        }),
        signal,
      });
      if (abortController === controller) abortController = null;

      let data: any;
      try { data = await res.json(); } catch {
        const errResult: CallResult = { status: 'error', statusCode: res.status, messages: [], headers: {}, trailers: {}, error: `Server returned ${res.status} ${res.statusText}`, durationMs: Math.round(performance.now() - start) };
        const errEntry: HistoryEntry = { id: id(), timestamp: now(), endpoint: st.request.endpoint, bodies: st.request.bodies, headers: st.request.headers, response: errResult };
        historyCache.put(errEntry.id, errEntry);
        saveHistoryToStorage();
        set(s => {
          const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, response: errResult } : t);
          const totalError = s.totalError + 1;
          saveTotals(s.totalOk, totalError);
          return { tabs, response: errResult, history: historyCache.values(), totalError };
        });
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
      set(s => {
        const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, response: result } : t);
        if (data.success) {
          const totalOk = s.totalOk + 1;
          saveTotals(totalOk, s.totalError);
          return { tabs, response: result, history: historyCache.values(), totalOk };
        } else {
          const totalError = s.totalError + 1;
          saveTotals(s.totalOk, totalError);
          return { tabs, response: result, history: historyCache.values(), totalError };
        }
      });
    } catch (err: any) {
      if (abortController === controller) abortController = null;
      if (err?.name === 'AbortError') {
        set(s => {
          const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, response: null } : t);
          return { tabs, response: null };
        });
        return;
      }
      const errResult: CallResult = { status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: err?.message || String(err), durationMs: Math.round(performance.now() - start) };
      const errEntry: HistoryEntry = { id: id(), timestamp: now(), endpoint: st.request.endpoint, bodies: st.request.bodies, headers: st.request.headers, response: errResult };
      historyCache.put(errEntry.id, errEntry);
      saveHistoryToStorage();
      set(s => {
        const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, response: errResult } : t);
        const totalError = s.totalError + 1;
        saveTotals(s.totalOk, totalError);
        return { tabs, response: errResult, history: historyCache.values(), totalError };
      });
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



