import { saveExtFor, withFamilyExt } from './tree';
import { apiPath } from './api-path';
import { create } from 'zustand';

rememberThemeUnder('grpctestify-theme');
const INITIAL_THEME: ThemeChoice = readChoice(readText(THEME_KEY));
import { countDatasetRefs, pruneRows, renameColumn, renameDatasetRefs } from './dataset-model';
import type { PlayStore, HistoryEntry, CallResult, CollectionItem, CollectionParsed, Tab, StoredTab, TabsStorage, Environment, ReflectResponse, GctfMeta, DocumentSummary, ExpectMessage } from './types';
import { ENVS_KEY, ACTIVE_ENV_KEY, TABS_KEY, SETTINGS_KEY, RECENT_ADDRESS_KEY, defaultAddressFor, dialledAddress } from './types';
import type { ClientSettings, WireProtocol } from './types';
import { LRUCache } from 'luvo/data/cache';
import { applyEvent, cancelJob, caseTitle, emptyRun, fileOfCase, followJob, jobReports, runRefusal, runningJobs, startJob, unsavedAmong } from './jobs';
import { buildMoved, loadedBuild } from './build-id';
import { timeoutSeconds } from './format';
import { schemaRequest } from './schema-request';
import { getSessionId } from './session';
import { loadRecent, pushRecent, saveRecent } from './history-list';
import { applyBindings, applyEnvironment, effectiveEnvironment, mergeEnvLists, resolvedNames, substituteEnv } from './env';
import { reflectOutcome, schemaKey } from './reflect-outcome';
import { addressForSave, protocolForSave } from './save-meta';
import { clampRow } from './dataset-row';
import { lineDiff } from 'luvo/data/diff';
import { nextCopyName } from './duplicate-name';
import { isSecretHeader } from './secret-headers';
import { readJson, readText, writeJson, writeText } from 'luvo/data/storage';
import { checkSummary, checkedAfterMove, mergeChecked, type CheckedFile } from './checked';
import { rememberedChoice } from './run-data';
import { errorExpectBody, expectBody } from './expect-model';
import { errorText } from './grpc-error';
import { addressDecision, chainAddressAt, type AddressDecision } from './address';
import { toCurl } from './curl-import';
import { httpUrl, isHttpRequest, requestFamily, splitEndpoint, suggestedFileName } from './http-endpoint';
import { callKindOf, switchCall, switchable } from './call-kind';
import { splitExtractName } from './extract-name';
import { callFailed } from './call-outcome';
import { connectionUsed } from './connection-source';
import { verdictResponse, verdictResult, type Verdict } from './jobs';
import { applyTheme, readChoice, rememberThemeUnder, THEME_KEY, watchSystemTheme, type ThemeChoice } from 'luvo/theme/themes';
import type { ReflectAttempt } from './reflect-outcome';
import { duplicateItem, moveItem } from './message-order';
import { parsedForStep } from './step-model';
import { labelFor, movedPath } from './move-paths';
import { previewSlot, tabHoldingCall } from './preview-slot';
import { count } from 'luvo/data/plural';
import { serverAnswered } from './answer-source';

function now() { return Date.now(); }
function id() { return Math.random().toString(36).slice(2, 9); }

const DEFAULT_BODY = '{}';
const DEFAULT_BODIES = [DEFAULT_BODY];
const historyCache = new LRUCache<string, HistoryEntry>(1000);
const abortControllers = new Map<string, AbortController>();
let reflectController: AbortController | null = null;
let reflectSeq = 0;
let openSeq = 0;
const REFLECT_TIMEOUT_MS = 30_000;

const EMPTY_REQUEST = { endpoint: '', headers: {}, bodies: DEFAULT_BODIES };

function bodiesFor(
  path: string | null | undefined,
  endpoint: string,
  bodies: string[],
): string[] {
  if (bodies.length > 0) return [...bodies];
  return isHttpRequest(path, endpoint) ? [] : [...DEFAULT_BODIES];
}

const initialBrowserEnvs: Environment[] = (() => {
  const stored = readJson<Environment[]>(ENVS_KEY, []);
  if (!Array.isArray(stored)) return [];
  const mine = stored.filter(e => e?.source !== 'project').map(e => ({ ...e, source: 'browser' as const }));
  if (mine.length !== stored.length) writeJson(ENVS_KEY, mine);
  return mine;
})();

export async function initProjectEnvs(envNames: string[]) {
  if (envNames.length === 0) return;
  const projectEnvs: Environment[] = [];
  for (const name of envNames) {
    try {
      const res = await fetch(`/api/project/env/${encodeURIComponent(name)}/merged`);
      if (!res.ok) continue;
      const data = await res.json();
      projectEnvs.push({
        name,
        source: 'project',
        address: data.address || undefined,
        variables: data.variables,
      });
    } catch {  }
  }
  useStore.setState(s => ({ projectEnvs, ...merged(projectEnvs, s.browserEnvs) }));
}

function merged(project: Environment[], browser: Environment[]) {
  return { environments: mergeEnvLists(project, browser).list };
}

function saveBrowserEnvs(browser: Environment[]) {
  writeJson(ENVS_KEY, browser);
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
    rawContent: null,
    rawOriginal: null,
    addressTouched: false,
    protocolTouched: false,
    runMode: 'execute',
  };
}

export function isPristineTab(tab: Tab): boolean {
  return tab.collectionPath === null
    && tab.rawContent === null
    && tab.endpoint.trim() === ''
    && Object.keys(tab.headers).length === 0
    && tab.bodies.length === 1
    && tab.bodies[0].trim() === DEFAULT_BODY
    && !tab.addressTouched;
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
    rawContent: overrides?.rawContent ?? state.rawContent,
    rawOriginal: overrides?.rawOriginal ?? state.rawOriginal,
    parseError: overrides?.parseError ?? state.parseError,
    staleOnDisk: overrides?.staleOnDisk ?? state.staleOnDisk,
    addressTouched: overrides?.addressTouched ?? state.addressTouched,
    address: overrides?.address ?? (state.addressTouched ? state.address : undefined),
    protocolTouched: overrides?.protocolTouched ?? state.protocolTouched,
    runMode: overrides?.runMode ?? state.runMode,
    ...overrides,
  };
}

export function activeEnvAddress(st: PlayStore): string | null {
  const env = st.activeEnvironment
    ? st.environments.find(e => e.name === st.activeEnvironment)
    : null;
  return env?.address ?? null;
}

export function fileMissing(st: PlayStore): boolean {
  if (!st.workspacePath) return false;
  if (st.collectionsRead !== 'ok') return false;
  return !st.collections.some(c => !c.is_dir && c.path === st.workspacePath);
}

const listedPathCache = new WeakMap<CollectionItem[], Set<string>>();

export function listedPaths(st: PlayStore): Set<string> | null {
  if (st.collectionsRead !== 'ok') return null;
  const known = listedPathCache.get(st.collections);
  if (known) return known;
  const paths = new Set(st.collections.filter(c => !c.is_dir).map(c => c.path));
  listedPathCache.set(st.collections, paths);
  return paths;
}

export function tabFileMissing(tab: Tab, listed: Set<string> | null): boolean {
  return listed !== null && tab.collectionPath !== null && !listed.has(tab.collectionPath);
}

export function contentUnread(st: PlayStore): boolean {
  return !!st.workspacePath && unreadPaths.has(st.workspacePath) && st.rawContent === null;
}

export function keepFromAnotherRoot(tabs: Tab[]): Tab[] {
  const kept = tabs.filter(t => t.collectionPath === null && (
    t.endpoint.trim() !== '' || t.bodies.some(b => b.trim() !== '') || (t.rawContent ?? '').trim() !== ''
  ));
  return kept;
}

async function askServer(url: string, init: RequestInit, whatFailed: string): Promise<Response> {
  try {
    return await fetch(url, init);
  } catch {
    throw new Error(`The workbench could not be reached — ${whatFailed}`);
  }
}

async function writeOrSay(url: string, init: RequestInit, fallback: string): Promise<void> {
  let res: Response;
  try {
    res = await fetch(url, init);
  } catch {
    throw new Error('The workbench could not be reached — nothing was written');
  }
  if (res.ok) return;
  const said = await res.text().catch(() => '');
  throw new Error(said.trim() || fallback);
}

async function postWrite(url: string, body: unknown): Promise<Response> {
  try {
    return await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  } catch {
    throw new Error('The workbench could not be reached — nothing was written');
  }
}

function writeToTab(
  set: (updater: (s: PlayStore) => Partial<PlayStore>) => void,
  tabId: string | null,
  tab: Partial<Tab>,
  panel: Partial<PlayStore>,
): void {
  set(s => {
    const tabs = s.tabs.map(t => (t.id === tabId ? { ...t, ...tab } : t));
    return s.activeTabId === tabId ? { tabs, ...panel } : { tabs };
  });
}

const projectEnvCache = new WeakMap<Environment, { env: Environment; names: string[] }>();
const NO_NAMES: string[] = [];

function projectEnvView(st: PlayStore): { env: Environment; names: string[] } | null {
  const name = st.projectDefaults?.activeEnv;
  if (!name) return null;
  const raw = st.projectEnvs.find(e => e.name === name);
  if (!raw) return null;
  const known = projectEnvCache.get(raw);
  if (known) return known;
  const env = effectiveEnvironment(raw)!;
  const view = { env, names: Object.keys(env.variables) };
  projectEnvCache.set(raw, view);
  return view;
}

const mergedBindings = new WeakMap<object, WeakMap<object, [string, string][]>>();

export function bindingsOf(st: Pick<PlayStore, 'workspacePath' | 'run' | 'executeBound'>): [string, string][] | undefined {
  const path = st.workspacePath;
  if (!path) return undefined;
  const fromRun = st.run.verdicts[path]?.extracted;
  const fromCall = st.executeBound[path];
  if (!fromRun?.length) return fromCall?.length ? fromCall : undefined;
  if (!fromCall?.length) return fromRun;
  let byCall = mergedBindings.get(fromRun);
  if (!byCall) {
    byCall = new WeakMap();
    mergedBindings.set(fromRun, byCall);
  }
  const known = byCall.get(fromCall);
  if (known) return known;
  const merged = new Map(fromRun);
  for (const [name, value] of fromCall) merged.set(name, value);
  const out: [string, string][] = [...merged];
  byCall.set(fromCall, out);
  return out;
}

function boundByRun(st: PlayStore): { endpoint: string; headers: Record<string, string>; bodies: string[] } {
  return applyBindings(st.request.endpoint, st.request.headers, st.request.bodies, bindingsOf(st));
}

export function copyNote(st: PlayStore): string {
  const said: string[] = [];
  if (st.activeEnvironment) said.push(`"${st.activeEnvironment}" values`);
  if (runBindingsFilled(st)) said.push('the names this file has bound');
  return said.length === 0 ? '' : ` — ${said.join(' and ')} filled in`;
}

function runBindingsFilled(st: PlayStore): boolean {
  const bound = boundByRun(st);
  return bound.endpoint !== st.request.endpoint
    || st.request.bodies.some((b, i) => bound.bodies[i] !== b)
    || Object.entries(st.request.headers).some(([k, v]) => bound.headers[k] !== v);
}

function exported(
  filled: { endpoint: string; headers: Record<string, string>; bodies: string[] },
  st: PlayStore,
): { endpoint: string; headers: Record<string, string>; bodies: string[] } {
  const env = projectCallEnv(st);
  if (!env) return filled;
  return {
    endpoint: substituteEnv(filled.endpoint, env),
    headers: Object.fromEntries(
      Object.entries(filled.headers).map(([k, v]) => [k, substituteEnv(v, env)]),
    ),
    bodies: filled.bodies.map(b => substituteEnv(b, env)),
  };
}

export function projectCallEnv(st: PlayStore): Environment | null {
  return projectEnvView(st)?.env ?? null;
}

export function projectEnvNames(st: PlayStore): string[] {
  return projectEnvView(st)?.names ?? NO_NAMES;
}

export function resolveProjectAddress(address: string, env: Environment | null): string {
  if (!address.includes('{{') || !env) return address;
  return substituteEnv(address, env);
}

export function answeredHere(st: PlayStore): string[] {
  const env = st.activeEnvironment
    ? st.environments.find(e => e.name === st.activeEnvironment)
    : null;
  return resolvedNames(
    [st.request.endpoint, ...Object.values(st.request.headers), ...st.request.bodies],
    bindingsOf(st),
    effectiveEnvironment(env),
  );
}

export function callAddress(st: PlayStore): string {
  return addressDecision({
    file: st.collectionParsed?.address || chainAddressAt(st.documents, st.activeStep) || null,
    fileFromChain: !st.collectionParsed?.address,
    typed: st.address,
    environment: activeEnvAddress(st),
    server: st.serverEnv.address ?? null,
    fallback: addressFallback(st),
  }).address;
}

function addressFallback(st: PlayStore): string {
  return requestFamily(st.workspacePath, st.request.endpoint) === 'httf'
    ? ''
    : defaultAddressFor(st.protocol);
}

export function addressSourceOf(st: PlayStore): AddressDecision['source'] {
  return addressDecision({
    file: st.collectionParsed?.address || chainAddressAt(st.documents, st.activeStep) || null,
    fileFromChain: !st.collectionParsed?.address,
    typed: st.address,
    environment: activeEnvAddress(st),
    server: st.serverEnv.address ?? null,
    fallback: addressFallback(st),
  }).source;
}

export function runAddressDecision(st: PlayStore) {
  const env = st.activeEnvironment
    ? st.environments.find(e => e.name === st.activeEnvironment)
    : null;
  return addressDecision({
    file: st.collectionParsed?.address || chainAddressAt(st.documents, st.activeStep) || null,
    fileFromChain: !st.collectionParsed?.address,
    typed: '',
    environment: env?.source === 'project' ? env.address ?? null : null,
    server: st.serverEnv.address ?? null,
    fallback: addressFallback(st),
  });
}

export function effectiveTls(st: PlayStore) {
  const env = st.activeEnvironment
    ? st.environments.find(e => e.name === st.activeEnvironment)
    : null;
  if (env && env.tls !== undefined) {
    return {
      tls: env.tls,
      tlsInsecure: env.tlsInsecure ?? true,
      tlsCa: env.tlsCa || '',
      tlsCert: env.tlsCert || '',
      tlsKey: env.tlsKey || '',
    };
  }
  return { tls: st.tls, tlsInsecure: st.tlsInsecure, tlsCa: st.tlsCa, tlsCert: st.tlsCert, tlsKey: st.tlsKey };
}

function headersEqual(a: Record<string, string>, b: Record<string, string>): boolean {
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  return keys.every(k => a[k] === b[k]);
}

const PARSED_FIELDS = [
  'asserts', 'extracts', 'meta_name', 'meta_tags', 'meta_owner', 'meta_summary', 'meta_links',
  'tls', 'options', 'bench', 'proto', 'dataset', 'attributes',
  'expect_responses', 'expect_error',
] as const satisfies readonly (keyof CollectionParsed)[];

function structuredDirty(endpoint: string, headers: Record<string, string>, bodies: string[], orig: CollectionParsed | null, parsed?: CollectionParsed | null): boolean {
  if (!orig) return false;
  if (
    endpoint !== orig.endpoint ||
    !headersEqual(headers, orig.headers) ||
    JSON.stringify(bodies) !== JSON.stringify(orig.bodies)
  ) return true;
  if (!parsed) return false;
  return PARSED_FIELDS.some(
    field => JSON.stringify(parsed[field] ?? null) !== JSON.stringify(orig[field] ?? null),
  );
}

export type RawReason = 'unreadable' | 'no-file' | 'edited' | null;

export function rawAuthorityReason(
  st: Pick<PlayStore, 'rawContent' | 'rawOriginal'> & { parseError?: string | null },
): RawReason {
  if (st.rawContent === null) return null;
  if (st.parseError) return 'unreadable';
  if (st.rawOriginal === null) return 'no-file';
  return st.rawContent !== st.rawOriginal ? 'edited' : null;
}

export function rawAuthorityRefusal(reason: RawReason): string | null {
  switch (reason) {
    case 'unreadable':
      return 'A section of this file could not be read — edit it in the source tab';
    case 'no-file':
      return 'This text has no file behind it yet — save it first';
    case 'edited':
      return 'The source tab has unsaved edits; save them first';
    default:
      return null;
  }
}

export function rawIsAuthoritative(
  st: Pick<PlayStore, 'rawContent' | 'rawOriginal'> & { parseError?: string | null },
): boolean {
  if (st.rawContent === null) return false;
  if (st.parseError) return true;
  if (st.rawOriginal === null) return true;
  return st.rawContent !== st.rawOriginal;
}

function addressDirty(st: Pick<PlayStore, 'address' | 'addressTouched' | 'workspaceOriginal'>): boolean {
  return st.addressTouched && !!st.workspaceOriginal && st.address !== (st.workspaceOriginal.address ?? '');
}

function protocolDirty(st: Pick<PlayStore, 'protocol' | 'protocolTouched' | 'workspaceOriginal'>): boolean {
  if (!st.protocolTouched || !st.workspaceOriginal) return false;
  return protocolForSave(st.workspaceOriginal, st.protocol, true)
    !== (st.workspaceOriginal.options?.protocol || undefined);
}

export function isRequestDirty(st: PlayStore): boolean {
  if (addressDirty(st) || protocolDirty(st)) return true;
  return structuredDirty(st.request.endpoint, st.request.headers, st.request.bodies, st.workspaceOriginal, st.collectionParsed);
}

export function formsAheadOfFile(st: PlayStore): boolean {
  return !!st.workspacePath && !rawIsAuthoritative(st) && isRequestDirty(st);
}

function withoutVerdict(st: PlayStore, path: string) {
  const bound = path in st.executeBound ? { executeBound: without(st.executeBound, path) } : {};
  if (!(path in st.run.verdicts)) return bound;
  const verdicts = { ...st.run.verdicts };
  delete verdicts[path];
  const cases = Object.fromEntries(
    Object.entries(st.run.cases).filter(([id]) => fileOfCase(id) !== path),
  );
  return { ...bound, run: { ...st.run, verdicts, cases } };
}

function without<T>(map: Record<string, T>, key: string): Record<string, T> {
  const next = { ...map };
  delete next[key];
  return next;
}

export function workspaceDirty(st: PlayStore): boolean {
  if (st.rawContent !== null && st.rawOriginal !== null && st.rawContent !== st.rawOriginal) return true;
  return isRequestDirty(st);
}

const dirtyByTab = new WeakMap<Tab, boolean>();

export function isTabDirty(tab: Tab): boolean {
  const known = dirtyByTab.get(tab);
  if (known !== undefined) return known;
  const dirty = tabIsDirty(tab);
  dirtyByTab.set(tab, dirty);
  return dirty;
}

function tabIsDirty(tab: Tab): boolean {
  if (tab.rawContent !== null && tab.rawOriginal !== null && tab.rawContent !== tab.rawOriginal) return true;
  return structuredDirty(tab.endpoint, tab.headers, tab.bodies, tab.collectionOriginal, tab.collectionParsed);
}

export function isActiveTabDirty(
  tab: Tab,
  st: Pick<
    PlayStore,
    'request' | 'rawContent' | 'rawOriginal' | 'workspaceOriginal' | 'collectionParsed' | 'address' | 'addressTouched'
    | 'protocol' | 'protocolTouched'
  >,
): boolean {
  if (addressDirty(st) || protocolDirty(st)) return true;
  return tabIsDirty({
    ...tab,
    endpoint: st.request.endpoint,
    headers: st.request.headers,
    bodies: st.request.bodies,
    rawContent: st.rawContent,
    rawOriginal: st.rawOriginal,
    collectionOriginal: st.workspaceOriginal,
    collectionParsed: st.collectionParsed,
  });
}

export type FileVersion = { mtime_ms: number; hash: string };

const fileVersions = new Map<string, FileVersion>();

const unreadPaths = new Set<string>();

function clearStale(set: (fn: (s: PlayStore) => Partial<PlayStore>) => void, path: string) {
  set(s => {
    const tabs = s.tabs.map(t => (t.collectionPath === path && t.staleOnDisk ? { ...t, staleOnDisk: false } : t));
    const active = tabs.find(t => t.id === s.activeTabId);
    return { tabs, staleOnDisk: active?.staleOnDisk ?? false };
  });
}

function markStale(set: (fn: (s: PlayStore) => Partial<PlayStore>) => void, path: string) {
  set(s => {
    const tabs = s.tabs.map(t => (t.collectionPath === path && !t.staleOnDisk ? { ...t, staleOnDisk: true } : t));
    const active = tabs.find(t => t.id === s.activeTabId);
    return { tabs, staleOnDisk: active?.staleOnDisk ?? false };
  });
}

function rememberVersion(path: string, version: FileVersion | undefined) {
  if (version) fileVersions.set(path, version);
}

async function refreshRawFromDisk(
  set: (updater: (s: PlayStore) => Partial<PlayStore>) => void,
  tabId: string | null,
  path: string,
) {
  const res = await fetch(`/api/collections/${apiPath(path)}`).catch(() => null);
  const data = res?.ok ? await res.json().catch(() => null) : null;
  if (typeof data?.content !== 'string') return;
  rememberVersion(path, data.version);
  writeToTab(
    set,
    tabId,
    { rawContent: data.content, rawOriginal: data.content },
    { rawContent: data.content, rawOriginal: data.content },
  );
}

export class SaveConflict extends Error {
  path: string;
  serverContent: string;
  serverVersion: FileVersion;

  constructor(path: string, serverContent: string, serverVersion: FileVersion) {
    super('File changed on disk since it was opened');
    this.name = 'SaveConflict';
    this.path = path;
    this.serverContent = serverContent;
    this.serverVersion = serverVersion;
  }
}

async function saveFailure(path: string, res: Response): Promise<Error> {
  const text = await res.text().catch(() => '');
  if (res.status === 409) {
    try {
      const body = JSON.parse(text);
      return new SaveConflict(path, body.content ?? '', body.version);
    } catch {
    }
  }
  return new Error(text || 'Save failed');
}

type CollectionRead = {
  parsed: CollectionParsed;
  documents: DocumentSummary[];
  parseError?: string | null;
  content?: string | null;
};

function handleParsed(tab: Tab): CollectionParsed {
  return {
    endpoint: tab.endpoint,
    address: '',
    headers: tab.headers,
    bodies: tab.bodies,
    asserts: [],
    extracts: {},
    meta_name: null,
    meta_tags: [],
    meta_owner: null,
    meta_summary: null,
    meta_links: [],
    tls: {},
    options: {},
    bench: {},
    proto: {},
    dataset: [],
    attributes: [],
    expect_responses: [],
    expect_error: null,
  };
}

async function fetchCollection(path: string): Promise<CollectionRead | null> {
  try {
    const res = await fetch(`/api/collections/${apiPath(path)}`);
    if (!res.ok) { unreadPaths.add(path); return null; }
    const data = await res.json();
    rememberVersion(path, data.version);
    if (!data.parsed) { unreadPaths.add(path); return null; }
    unreadPaths.delete(path);
    return {
      parsed: data.parsed,
      documents: data.documents ?? [],
      parseError: data.parse_error ?? null,
      content: typeof data.content === 'string' ? data.content : null,
    };
  } catch {
    unreadPaths.add(path);
    return null;
  }
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
    documents: tab.documents ?? [],
    activeStep: 0,
    datasetRow: 0,
  headParsed: null,
    workspaceOriginal: tab.collectionOriginal,
    rawContent: tab.rawContent,
    rawOriginal: tab.rawOriginal,
    parseError: tab.parseError ?? null,
    staleOnDisk: tab.staleOnDisk ?? false,
    selectedCollection: tab.collectionPath,
    addressTouched: tab.addressTouched ?? false,
    ...(tab.addressTouched && tab.address !== undefined ? { address: tab.address } : {}),
    protocolTouched: tab.protocolTouched ?? false,
    runMode: tab.runMode ?? 'execute',
  };
}

export const MAX_STORED_RAW = 256 * 1024;

export function serializeTab(t: Tab): StoredTab {
  const unsaved = !t.collectionPath && t.rawContent !== null && t.rawContent.length <= MAX_STORED_RAW;
  return {
    i: t.id, l: t.label, e: t.endpoint, h: t.headers, b: t.bodies, c: t.collectionPath,
    ...(unsaved ? { r: t.rawContent! } : {}),
    ...(t.runMode === 'run' ? { m: 'run' as const } : {}),
    ...(!t.collectionPath && t.addressTouched && t.address ? { d: t.address } : {}),
  };
}

export function deserializeTab(s: StoredTab): Tab {
  const tId = s.i || id();
  const endpoint = s.e || '';
  const http = isHttpRequest(s.c, endpoint);
  const stored = s.b && s.b.length > 0 ? s.b : null;
  const placeholder = !!s.c && http && stored?.length === 1 && stored[0].trim() === DEFAULT_BODY;
  const bodies = stored && !placeholder ? stored : http ? [] : [...DEFAULT_BODIES];
  return {
    id: tId,
    label: s.l || 'Untitled',
    endpoint,
    headers: s.h || {},
    bodies,
    response: null,
    requestTab: s.r ? 'source' : 'body',
    gctfTab: 'request',
    responseTab: 'response',
    collectionPath: s.c || null,
    collectionParsed: null,
    collectionOriginal: null,
    rawContent: s.r ?? null,
    rawOriginal: null,
    runMode: s.m === 'run' ? 'run' : 'execute',
    ...(s.d ? { address: s.d, addressTouched: true } : {}),
  };
}

let storedTabsRoot: string | null = null;
let tabsRoot = '';

function saveTabsToStorage(tabs: Tab[], activeTabId: string | null) {
  try {
    const stored: TabsStorage = {
      t: tabs.map(serializeTab),
      a: activeTabId,
      ...(tabsRoot ? { r: tabsRoot } : {}),
    };
    localStorage.setItem(TABS_KEY, JSON.stringify(stored));
  } catch {  }
}

function loadTabsFromStorage(): { tabs: Tab[]; activeTabId: string | null } {
  try {
    const raw = localStorage.getItem(TABS_KEY);
    if (!raw) return { tabs: [defaultTab()], activeTabId: null };
    const stored: TabsStorage = JSON.parse(raw);
    storedTabsRoot = stored.r ?? null;
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
  address: '',
  protocol: 'grpc',
  tls: false,
  tlsInsecure: true,
  tlsCa: '',
  tlsCert: '',
  tlsKey: '',
  requestTimeoutMs: 0,
};

function loadSettings(): ClientSettings {
  return { ...DEFAULT_SETTINGS, ...readJson<Partial<ClientSettings>>(SETTINGS_KEY, {}) };
}

function saveSettings(s: ClientSettings) {
  writeJson(SETTINGS_KEY, s);
}

function clientSettings(s: PlayStore): ClientSettings {
  return {
    address: s.address,
    protocol: s.protocol,
    tls: s.tls,
    tlsInsecure: s.tlsInsecure,
    tlsCa: s.tlsCa,
    tlsCert: s.tlsCert,
    tlsKey: s.tlsKey,
    requestTimeoutMs: s.requestTimeoutMs,
  };
}

function saveTotals(ok: number, error: number) {
  writeJson(TOTALS_KEY, { ok, error });
}

function loadTotals(): { ok: number; error: number } {
  const held = readJson<{ ok?: unknown; error?: unknown }>(TOTALS_KEY, {});
  return {
    ok: typeof held.ok === 'number' ? held.ok : 0,
    error: typeof held.error === 'number' ? held.error : 0,
  };
}

const initTabs = loadTabsFromStorage();
const initActive = initTabs.activeTabId || initTabs.tabs[0]?.id || null;
const initTab = initTabs.tabs.find(t => t.id === initActive) || initTabs.tabs[0];

const initialSettings = loadSettings();

function originalFor(st: PlayStore, target: string): string | undefined {
  if (st.collections.some(c => !c.is_dir && c.path === target)) return target;
  return st.workspacePath ?? undefined;
}

function cleanMeta(meta: GctfMeta | undefined): GctfMeta | undefined {
  if (!meta) return undefined;
  const out: GctfMeta = {};
  if (meta.name?.trim()) out.name = meta.name.trim();
  if (meta.summary?.trim()) out.summary = meta.summary.trim();
  if (meta.owner?.trim()) out.owner = meta.owner.trim();
  const tags = (meta.tags ?? []).map(t => t.trim()).filter(Boolean);
  if (tags.length > 0) out.tags = tags;
  const links = (meta.links ?? []).map(t => t.trim()).filter(Boolean);
  if (links.length > 0) out.links = links;
  return Object.keys(out).length > 0 ? out : undefined;
}

function parsedOrShell(st: PlayStore): CollectionParsed {
  return st.collectionParsed ?? {
    endpoint: st.request.endpoint, address: st.address, headers: st.request.headers,
    bodies: st.request.bodies, asserts: [], extracts: {},
    meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
    tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
    expect_responses: [], expect_error: null,
  };
}

function blankExpect(): ExpectMessage {
  return { body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] };
}

function withParsed(st: PlayStore, next: CollectionParsed) {
  const tabs = st.tabs.map(t => t.id === st.activeTabId ? { ...t, collectionParsed: next } : t);
  return { tabs, collectionParsed: next };
}

export type SavePayloadSource = Pick<
  PlayStore,
  'collectionParsed' | 'protocol' | 'protocolTouched' | 'request' | 'address' | 'addressTouched'
>;

export function structuredSave(st: SavePayloadSource) {
  const p = st.collectionParsed;

  const options: [string, string][] = Object.entries(p?.options ?? {})
    .filter(([k]) => k !== 'protocol');
  const chosenProtocol = protocolForSave(p, st.protocol, st.protocolTouched);
  if (chosenProtocol) options.push(['protocol', chosenProtocol]);
  const optionsChanged =
    JSON.stringify(Object.fromEntries(options)) !== JSON.stringify(p?.options ?? {});

  const address = addressForSave(p, st.address, st.addressTouched);

  return {
    endpoint: st.request.endpoint,
    parallel: p?.parallel ?? false,
    bodies: st.request.bodies,
    bodies_stream: p?.bodies_stream ?? false,
    headers: namedHeaders(st.request.headers),
    address,
    options: p || optionsChanged || options.length > 0 ? options : undefined,
    asserts: p ? p.asserts : undefined,
    extract: p ? Object.entries(p.extracts).map(([name, expr]) => {
      const kind = p.extract_types?.[name];
      return [kind ? `${name}:${kind}` : name, expr] as [string, string];
    }) : undefined,
    tls: p ? Object.entries(p.tls) : undefined,
    proto: p ? Object.entries(p.proto) : undefined,
    bench: p ? Object.entries(p.bench) : undefined,
    dataset: p ? pruneRows(p.dataset) : undefined,
    meta: p ? (metaOf(p) ?? {}) : undefined,
    expect: p ? { responses: p.expect_responses ?? [], error: p.expect_error ?? null } : undefined,
  };
}

function namedHeaders(headers: Record<string, string>): Record<string, string> | undefined {
  const named = Object.fromEntries(Object.entries(headers).filter(([k]) => k.trim() !== ''));
  return Object.keys(named).length > 0 ? named : undefined;
}

function metaOf(p: CollectionParsed): GctfMeta | undefined {
  const meta: GctfMeta = {};
  if (p.meta_name) meta.name = p.meta_name;
  if (p.meta_summary) meta.summary = p.meta_summary;
  if (p.meta_owner) meta.owner = p.meta_owner;
  if (p.meta_tags.length > 0) meta.tags = p.meta_tags;
  if (p.meta_links?.length) meta.links = p.meta_links;
  return Object.keys(meta).length > 0 ? meta : undefined;
}

function conflictOf(err: SaveConflict, mine: string, raw: boolean) {
  return { path: err.path, mine, theirs: err.serverContent, raw };
}

async function previewOfSave(st: PlayStore, path: string): Promise<string> {
  try {
    const res = await fetch('/api/preview-structured', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ...structuredSave(st),
        path,
        original_path: st.workspacePath ?? undefined,
        document_index: st.activeStep,
      }),
    });
    if (!res.ok) return renderedMine(st);
    const data = await res.json();
    return typeof data?.content === 'string' && data.content !== '' ? data.content : renderedMine(st);
  } catch {
    return renderedMine(st);
  }
}

function renderedMine(st: PlayStore): string {
  const body = structuredSave(st);
  return [
    body.address ? `--- ADDRESS ---\n${body.address}` : '',
    `--- ENDPOINT ---\n${body.endpoint}`,
    ...(body.bodies ?? []).map(b => `--- REQUEST ---\n${b}`),
  ].filter(Boolean).join('\n\n');
}

let stopFollowing: (() => void) | null = null;

type SetState = (partial: Partial<PlayStore> | ((s: PlayStore) => Partial<PlayStore>)) => void;
type GetState = () => PlayStore;

async function formatted(content: string, fileName: string): Promise<string> {
  try {
    const res = await fetch('/api/fmt', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content, file_name: fileName }),
    });
    if (!res.ok) return content;
    const data = await res.json();
    return data.changed ? data.formatted : content;
  } catch {
    return content;
  }
}

function setVerdict(set: SetState, path: string, verdict: Verdict) {
  set(s => ({ run: { ...s.run, verdicts: { ...s.run.verdicts, [path]: verdict } } }));
}

function follow(jobId: string, set: SetState, get: GetState): () => void {
  return followJob(
    jobId,
    e => set(s => {
      const run = { ...applyEvent(s.run, e), lost: 0 };
      const tabs = s.tabs.map(t => {
        const verdict = t.collectionPath ? run.verdicts[t.collectionPath] : undefined;
        if (!verdict || verdict === s.run.verdicts[t.collectionPath ?? '']) return t;
        const answered = verdictResult(verdict);
        const carries = !!answered
          && ((answered.assertions?.length ?? 0) > 0 || answered.messages.length > 0 || !!answered.error);
        return carries ? { ...t, response: answered } : t;
      });
      const active = tabs.find(t => t.id === s.activeTabId);
      const dialled = typeof e.address === 'string' && e.address.trim() !== ''
        ? { lastCallAddress: e.address }
        : {};
      const changed = tabs.some((t, i) => t !== s.tabs[i]);
      if (!changed) return { run, ...dialled };
      return {
        run,
        tabs,
        ...dialled,
        ...(active ? { response: active.response } : {}),
      };
    }),
    async final => {
      set(s => ({
        run: {
          ...s.run,
          finished: true,
          lost: 0,
          outcome: final?.status ?? null,
          durationMs: final?.duration_ms ?? s.run.durationMs,
        },
        runJobId: null,
        runError: final ? get().runError
          : 'The stream ended and the server no longer has this job',
      }));
      const files = await jobReports(jobId);
      set({ lastReports: { jobId, files } });
    },
    attempt => set(s => ({ run: { ...s.run, lost: attempt } })),
  );
}

function applied(choice: ThemeChoice) {
  const themeMode = applyTheme(choice);
  try { localStorage.setItem(THEME_KEY, JSON.stringify(choice)); } catch { /* private mode */ }
  return { ...choice, themeMode };
}

const rememberedData = rememberedChoice(readJson<unknown>('play.run.data', null));

async function diskVersions(paths: string[]): Promise<Map<string, FileVersion>> {
  try {
    const res = await fetch('/api/versions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ paths }),
    });
    if (!res.ok) return new Map();
    const data = (await res.json()) as Record<string, FileVersion | null>;
    return new Map(Object.entries(data).filter((e): e is [string, FileVersion] => e[1] !== null));
  } catch {
    return new Map();
  }
}

export const useStore = create<PlayStore>((set, get) => ({
  address: initialSettings.address,
  protocol: initialSettings.protocol,
  tls: initialSettings.tls,
  tlsInsecure: initialSettings.tlsInsecure,
  tlsCa: initialSettings.tlsCa,
  tlsCert: initialSettings.tlsCert,
  tlsKey: initialSettings.tlsKey,
  requestTimeoutMs: initialSettings.requestTimeoutMs,
  startupNote: null,
  addressTouched: false,
  protocolTouched: false,
  collections: [],
  collectionsRead: 'pending',
  projectRoot: null,
  projectRootAbs: null,
  collectionsDir: null,
  projectEnvNames: [],
  projectDefaults: null,

  tabs: initTabs.tabs,
  activeTabId: initActive,

  workspacePath: initTab?.collectionPath || null,
  workspaceOriginal: null,
  selectedCollection: initTab?.collectionPath || null,
  collectionParsed: null,
  rawContent: initTab?.rawContent ?? null,
  rawOriginal: null,
  rawError: null,
  parseError: null,
  staleOnDisk: false,
  request: initTab ? { endpoint: initTab.endpoint, headers: initTab.headers, bodies: initTab.bodies } : { ...EMPTY_REQUEST },
  requestTab: initTab?.rawContent ? 'source' : 'body',
  gctfTab: 'request',
  response: null,
  responseTab: 'response',

  history: [],
  totalOk: loadTotals().ok,
  totalError: loadTotals().error,
  version: '',
  workspaceName: '',
  sharesPath: '.grpctestify/shares',
  sessionId: getSessionId(),
  recentAddresses: loadRecent(RECENT_ADDRESS_KEY),
  changedPaths: null,
  changedSince: readText('play.changed.since', '') || null,
  changedAvailable: false,
  serverEnv: {},
  lastCallAddress: null,
  runError: null,
  palette: INITIAL_THEME.palette,
  mode: INITIAL_THEME.mode,
  themeMode: applyTheme(INITIAL_THEME),
  reflectionMethods: [],
  reflectStatus: 'idle',
  reflectedAt: null,
  reflectError: null,
  reflectedAddress: null,
  serverHealthy: true,
  collectionsMtime: 0,
  sidebarVisible: true,
  saveConflict: null,
  revealLine: null,
  drawerOpen: false,
  jqSeed: null,
  envManager: null,
  responseMessage: 0,
  benchBaseline: null,
  benchBaselinePath: null,
  benchPaths: [],
  benchBaselinePartial: false,
  benchOverUnsaved: [],
  benchComparison: null,
  problemCount: 0,
  diagnostics: [],
  diagnosedText: null,
  sidebarTab: 'collections',
  documents: [],
  activeStep: 0,
  datasetRow: 0,
  executeBound: {},
  headParsed: null,
  showHotkeyHelp: false,
  runStatus: 'idle',
  runMode: 'execute',
  buildMoved: false,
  browserEnvs: initialBrowserEnvs,
  projectEnvs: [],
  environments: initialBrowserEnvs,
  activeEnvironment: readText(ACTIVE_ENV_KEY) || null,

  setAddress: (v) => { set({ address: v, addressTouched: true }); saveSettings(clientSettings(get())); },
  nameAddressInFile: () => {
    const address = callAddress(get()).trim();
    if (address === '') return false;
    get().setAddress(address);
    return true;
  },
  rememberAddress: (v) => set(s => {
    const next = pushRecent(s.recentAddresses, v);
    saveRecent(RECENT_ADDRESS_KEY, next);
    return { recentAddresses: next };
  }),
  setProtocol: (v) => {
    set({ protocol: v, protocolTouched: true });
    saveSettings(clientSettings(get()));
  },
  setTls: (v) => { set({ tls: v }); saveSettings(clientSettings(get())); },
  setTlsInsecure: (v) => { set({ tlsInsecure: v }); saveSettings(clientSettings(get())); },
  setTlsCa: (v) => { set({ tlsCa: v }); saveSettings(clientSettings(get())); },
  setTlsCert: (v) => { set({ tlsCert: v }); saveSettings(clientSettings(get())); },
  setTlsKey: (v) => { set({ tlsKey: v }); saveSettings(clientSettings(get())); },
  setRequestTimeoutMs: (v) => { set({ requestTimeoutMs: v }); saveSettings(clientSettings(get())); },

  setEndpoint: (v) => set(s => {
    const drops = !s.workspacePath
      && isHttpRequest(null, v)
      && !isHttpRequest(null, s.request.endpoint)
      && s.request.bodies.length === 1
      && s.request.bodies[0] === DEFAULT_BODY;
    const bodies = drops ? [] : s.request.bodies;
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, endpoint: v, bodies } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, endpoint: v, bodies } };
  }),

  setCallKind: (kind) => set(s => {
    const kindNow = callKindOf(s.workspacePath, s.request.endpoint);
    if (kindNow === kind || !switchable(s.workspacePath).can) return {};

    const tab = s.tabs.find(t => t.id === s.activeTabId);
    const moved = switchCall({
      to: kind,
      endpoint: s.request.endpoint,
      other: tab?.otherEndpoint ?? '',
      address: s.address,
      addressTouched: s.addressTouched,
      grpcDefault: defaultAddressFor(s.protocol),
    });

    const bodies = bodiesFor(s.workspacePath, moved.endpoint, []);
    const tabs = s.tabs.map(t => t.id === s.activeTabId
      ? { ...t, endpoint: moved.endpoint, otherEndpoint: moved.other, bodies }
      : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return {
      tabs,
      address: moved.address,
      request: { ...s.request, endpoint: moved.endpoint, bodies },
    };
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

  moveRequestBody: (from, to) => set(s => {
    const bodies = moveItem(s.request.bodies, from, to);
    if (bodies === s.request.bodies) return {};
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, bodies } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs, request: { ...s.request, bodies } };
  }),

  duplicateRequestBody: (idx) => set(s => {
    const bodies = duplicateItem(s.request.bodies, idx);
    if (bodies === s.request.bodies) return {};
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

  setRunMode: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, runMode: v } : t);
    saveTabsToStorage(tabs, s.activeTabId);
    return { runMode: v, tabs };
  }),

  revealInRaw: (line) => {
    const st = get();
    if (st.requestTab !== 'source') st.setRequestTab('source');
    set({ revealLine: line });
  },

  clearReveal: () => set({ revealLine: null }),

  setDrawerOpen: (open) => set({ drawerOpen: open }),
  openJq: (expr) => set(s => ({
    drawerOpen: true,
    jqSeed: { expr, n: (s.jqSeed?.n ?? 0) + 1 },
  })),

  dismissStartupNote: () => set({ startupNote: null }),
  openEnvManager: (defineVar = null, value) => set({ envManager: { defineVar, ...(value === undefined ? {} : { value }) } }),
  closeEnvManager: () => set({ envManager: null }),
  setResponseMessage: (index) => set({ responseMessage: index }),

  setProblemCount: (n) => set(s => (s.problemCount === n ? s : { problemCount: n })),
  setDiagnostics: (list, text) => set(s => (
    s.diagnosedText === text && s.diagnostics === list ? s : { diagnostics: list, diagnosedText: text }
  )),
  showSidebarTab: (tab) => set({ sidebarTab: tab, sidebarVisible: true }),

  setGctfTab: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, gctfTab: v } : t);
    return { tabs, gctfTab: v };
  }),

  setResponseTab: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, responseTab: v } : t);
    return { tabs, responseTab: v };
  }),

  setCollections: (v) => set({ collections: v }),

  layout: (readText('play.layout', 'rows') as PlayStore['layout']),
  setLayout: (layout) => { writeText('play.layout', layout); set({ layout }); },
  visibleFiles: [],
  setVisibleFiles: (paths) => set(s => (
    s.visibleFiles.length === paths.length && s.visibleFiles.every((p, i) => p === paths[i])
      ? s
      : { visibleFiles: paths }
  )),
  runFilter: 'all',
  runReason: null,
  setRunFilter: (mode) => set({ runFilter: mode, runReason: null }),
  setRunReason: (reason) => set({ runReason: reason, runFilter: 'all' }),
  reportFormats: readJson<string[]>('play.run.reports', []),
  toggleReportFormat: (format) => set(s => {
    const next = s.reportFormats.includes(format)
      ? s.reportFormats.filter(f => f !== format)
      : [...s.reportFormats, format];
    writeJson('play.run.reports', next);
    return { reportFormats: next };
  }),
  lastReports: { jobId: '', files: [] },
  runScope: readText('play.run.scope', 'file') as PlayStore['runScope'],
  runData: rememberedData?.path ?? null,
  runDataColumns: rememberedData?.columns ?? [],
  setRunData: (path, columns) => {
    const held = path === null ? [] : (columns ?? []);
    writeJson('play.run.data', path === null ? null : { path, columns: held });
    set({ runData: path, runDataColumns: held });
  },
  setRunScope: (scope) => { writeText('play.run.scope', scope); set({ runScope: scope }); },
  run: emptyRun(),
  runJobId: null,

  startRun: async (paths, upToStep) => {
    if (paths.length === 0) return;
    stopFollowing?.();
    set({
      run: { ...emptyRun(), total: paths.length, upToStep },
      runJobId: null,
      runFilter: 'all',
      runReason: null,
      runError: null,
    });
    let job;
    try {
      job = await startJob(paths, upToStep, 'run', get().reportFormats, get().runData);
    } catch (e: any) {
      const st = get();
      const open = st.tabs.map(t => t.collectionPath).filter((p): p is string => !!p);
      const refusal = runRefusal(String(e?.message || e), open);
      set({ run: { ...emptyRun(), finished: true }, runError: refusal.text });
      if (refusal.path !== null) await get().refreshCollections();
      const active = st.tabs.find(t => t.id === st.activeTabId);
      const about = refusal.path !== null
        ? refusal.path
        : (paths.length === 1 ? paths[0] : null);
      if (about !== null && active && active.collectionPath === about) {
        const answer: CallResult = {
          status: 'error', statusCode: null, messages: [], headers: {}, trailers: {},
          error: refusal.text, durationMs: null, sent: false,
        };
        set(s => {
          const patch: Partial<PlayStore> = { tabs: s.tabs.map(t => t.id === active.id ? { ...t, response: answer } : t) };
          if (s.activeTabId === active.id) patch.response = answer;
          return patch;
        });
      }
      return;
    }
    set({ runJobId: job.id, lastReports: { jobId: job.id, files: [] } });
    stopFollowing = follow(job.id, set, get);
  },

  startBench: async (target) => {
    const paths = Array.isArray(target) ? target : [target];
    const path = paths[0];
    stopFollowing?.();
    const sameFile = get().run.kind === 'bench' && get().benchBaselinePath === path;
    const previous = sameFile ? get().run.benchReport : get().benchBaseline;
    const previousPartial = sameFile
      ? get().run.outcome === 'cancelled'
      : get().benchBaselinePartial;
    set({
      run: { ...emptyRun(), kind: 'bench', total: paths.length },
      runJobId: null,
      runFilter: 'all',
      runReason: null,
      benchBaseline: previous ?? null,
      benchBaselinePath: path,
      benchPaths: paths,
      benchBaselinePartial: previous ? previousPartial : false,
      benchComparison: null,
      benchOverUnsaved: unsavedAmong(paths, get().tabs.map(t => ({ path: t.collectionPath, dirty: isTabDirty(t) }))),
      runError: null,
    });
    let job;
    try {
      job = await startJob(paths, undefined, 'bench');
    } catch (e: any) {
      const open = get().tabs.map(t => t.collectionPath).filter((p): p is string => !!p);
      set({ run: { ...emptyRun(), kind: 'bench', finished: true }, runError: runRefusal(String(e?.message || e), open).text });
      return;
    }
    set({ runJobId: job.id });
    stopFollowing = follow(job.id, set, get);
  },

  adoptRunningJob: async () => {
    if (get().runJobId) return;
    const [job] = await runningJobs();
    if (!job || get().runJobId) return;
    stopFollowing?.();
    set({
      run: { ...emptyRun(), kind: job.kind === 'bench' ? 'bench' : 'run', total: job.total },
      runJobId: job.id,
      runFilter: 'all',
      runReason: null,
      runError: null,
      lastReports: { jobId: job.id, files: [] },
      ...(job.kind === 'bench' ? { benchPaths: job.paths ?? [], benchBaselinePath: job.paths?.[0] ?? null } : {}),
    });
    stopFollowing = follow(job.id, set, get);
  },

  compareBench: async () => {
    const { benchBaseline, run } = get();
    const current = run.benchReport;
    if (!benchBaseline || !current) return;
    try {
      const res = await fetch('/api/bench/compare', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ baseline: benchBaseline, current }),
      });
      if (!res.ok) return;
      set({ benchComparison: await res.json() });
    } catch { /* the numbers are still on screen without the comparison */ }
  },

  cancelRun: async () => {
    const id = get().runJobId;
    if (id) await cancelJob(id);
  },

  setCollectionParsed: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, collectionParsed: v } : t);
    return { tabs, collectionParsed: v };
  }),

  setPalette: (v) => set(s => applied({ palette: v, mode: s.mode })),
  setMode: (v) => set(s => applied({ palette: s.palette, mode: v })),

  setReflectionMethods: (v) => set({ reflectionMethods: v, reflectStatus: v.length > 0 ? 'ok' : 'error' }),

  reflect: async () => {
    const { protocol, workspacePath } = get();
    const { tls, tlsInsecure, tlsCa, tlsCert, tlsKey } = effectiveTls(get());
    const address = callAddress(get());
    const hadMethods = get().reflectionMethods.length > 0;
    set({ reflectStatus: 'loading', reflectError: null });
    if (reflectController) reflectController.abort();
    reflectController = new AbortController();
    const reflector = reflectController;
    const mine = ++reflectSeq;
    let timedOut = false;
    const timeoutId = setTimeout(() => { timedOut = true; reflector.abort(); }, REFLECT_TIMEOUT_MS);
    const settle = (attempt: Omit<ReflectAttempt, 'timedOut' | 'superseded' | 'hadMethods' | 'seconds'>) => {
      const out = reflectOutcome({ ...attempt, timedOut, hadMethods, superseded: reflectController !== reflector && reflectController !== null, seconds: REFLECT_TIMEOUT_MS / 1000 });
      if (out.status === 'loading') return;
      set({
        reflectStatus: out.status,
        reflectError: out.error,
        reflectedAddress: schemaKey({ address, protocol, collectionPath: workspacePath }),
        reflectedAt: out.status === 'ok' ? Date.now() : null,
        ...(out.clearMethods ? { reflectionMethods: [] } : {}),
      });
    };
    try {
      const res = await fetch('/api/reflect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          address,
          tls: tls || undefined,
          tls_insecure: tls ? tlsInsecure : undefined,
          tls_ca: tls ? (tlsCa || undefined) : undefined,
          tls_cert: tls ? (tlsCert || undefined) : undefined,
          tls_key: tls ? (tlsKey || undefined) : undefined,
          collection_path: workspacePath || undefined,
          protocol: protocol || undefined,
          timeout_seconds: timeoutSeconds(get().requestTimeoutMs) || undefined,
        }),
        signal: reflector.signal,
      });
      if (reflectController === reflector) reflectController = null;
      if (!res.ok) {
        settle({ aborted: false, ok: false, status: res.status, statusText: res.statusText });
        return;
      }
      const data: ReflectResponse = await res.json();
      const methods = data.error ? [] : (data.services ?? []).flatMap(s => (s.methods ?? []).map(m => ({
        name: m.name,
        fullName: m.full_name,
        service: s.name,
        clientStreaming: m.client_streaming,
        serverStreaming: m.server_streaming,
      })));
      if (methods.length > 0 && mine === reflectSeq) set({ reflectionMethods: methods });
      settle({ aborted: false, ok: true, reported: data.error ?? null, methodCount: methods.length });
    } catch (err: any) {
      if (reflectController === reflector) reflectController = null;
      const aborted = err?.name === 'AbortError';
      settle({ aborted, transportError: aborted ? null : (err?.message || 'could not reach the server') });
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
    const state = get();
    if (!config) {
      const live = (tab: Tab): Tab => tab.id !== state.activeTabId ? tab : {
        ...tab,
        endpoint: state.request.endpoint,
        headers: state.request.headers,
        bodies: state.request.bodies,
        rawContent: state.rawContent,
        collectionPath: state.workspacePath,
        addressTouched: state.addressTouched,
      };
      const active = state.tabs.find(t => t.id === state.activeTabId);
      const reuse = active && isPristineTab(live(active))
        ? active
        : state.tabs.find(t => t.id !== state.activeTabId && isPristineTab(t));
      if (reuse) {
        if (reuse.id !== state.activeTabId) get().setActiveTab(reuse.id);
        return reuse.id;
      }
    }
    const newTab = { ...defaultTab(), ...config, id: id(), response: null };
    let existing = state.tabs;
    if (existing.length >= MAX_TABS) {
      const spare = existing.find(t => t.id !== state.activeTabId && !isTabDirty(t));
      if (!spare) return null;
      existing = existing.filter(t => t.id !== spare.id);
    }
    const tabs = [...existing, newTab];
    saveTabsToStorage(tabs, newTab.id);
    set({
      tabs,
      activeTabId: newTab.id,
      ...loadTab(newTab),
    });
    return newTab.id;
  },

  moveTab: (from, to) => set(s => {
    const tabs = moveItem(s.tabs, from, to);
    if (tabs === s.tabs) return {};
    saveTabsToStorage(tabs, s.activeTabId);
    return { tabs };
  }),

  removeTab: (tabId) => {
    const state = get();
    const idx = state.tabs.findIndex(t => t.id === tabId);
    if (idx === -1) return;
    const remaining = state.tabs.filter(t => t.id !== tabId);
    const tabs = remaining.length > 0 ? remaining : [defaultTab()];
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

  focusHeldCall: (call) => {
    const state = get();
    const already = tabHoldingCall(state.tabs, call);
    if (!already) return false;
    const tabs = already.isPreview
      ? state.tabs.map(t => (t.id === already.id ? { ...t, isPreview: false } : t))
      : state.tabs;
    saveTabsToStorage(tabs, already.id);
    const focused = tabs.find(t => t.id === already.id)!;
    set({ tabs, activeTabId: already.id, ...loadTab(focused) });
    return true;
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

  loadCollection: async (path: string, options?: { pin?: boolean }) => {
    const state = get();
    const pin = options?.pin ?? false;
    const fromRun = verdictResponse(state.run.verdicts[path]);

    const existing = state.tabs.find(t => t.collectionPath === path);
    if (existing) {
      if (pin && existing.isPreview) {
        const tabs = state.tabs.map(t => (t.id === existing.id ? { ...t, isPreview: false } : t));
        saveTabsToStorage(tabs, existing.id);
        set({ tabs });
      }
      get().setActiveTab(existing.id);
      return true;
    }

    const mine = ++openSeq;
    const read = await fetchCollection(path);
    if (!read) return false;
    const superseded = mine !== openSeq;
    const p = read.parsed;
    const label = path.split('/').pop() || path;
    const newTab: Tab = {
      ...defaultTab(),
      id: id(),
      label,
      endpoint: p.endpoint,
      headers: p.headers,
      bodies: bodiesFor(path, p.endpoint, p.bodies),
      collectionPath: path,
      collectionParsed: p,
      collectionOriginal: p,
      documents: read.documents,
      requestTab: read.parseError ? 'source' : 'body',
      gctfTab: read.parseError ? 'raw' : 'request',
      parseError: read.parseError ?? null,
      ...(read.parseError && read.content !== null && read.content !== undefined
        ? { rawContent: read.content, rawOriginal: read.content }
        : {}),
      responseTab: fromRun?.assertions?.length ? 'assertions' : 'response',
      response: fromRun,
      isPreview: !pin,
    };
    const base = get().tabs;
    const previewIdx = superseded ? -1 : previewSlot(base);
    const tabs = previewIdx >= 0 && !pin
      ? base.map((t, i) => (i === previewIdx ? newTab : t))
      : [...base, newTab];
    if (superseded) {
      saveTabsToStorage(tabs, get().activeTabId);
      set({ tabs });
      return true;
    }
    saveTabsToStorage(tabs, newTab.id);
    set({ tabs, activeTabId: newTab.id, ...loadTab(newTab) });
    return true;
  },

  hydrateStaleTabs: async () => {
    const stale = get().tabs.flatMap(t =>
      t.collectionPath && !t.collectionParsed ? [{ id: t.id, path: t.collectionPath }] : []
    );
    await Promise.all(stale.map(async ({ id: tabId, path }) => {
      const read = await fetchCollection(path);
      if (!read) {
        set(s => ({
          tabs: s.tabs.map(t => {
            if (t.id !== tabId || t.collectionParsed) return t;
            const handle = handleParsed(t);
            return { ...t, collectionParsed: handle, collectionOriginal: handle };
          }),
        }));
        const updated = get().tabs.find(t => t.id === tabId);
        if (updated && get().activeTabId === tabId) set(loadTab(updated));
        return;
      }
      const parsed = read.parsed;
      set(s => {
        const tabs = s.tabs.map(t =>
          t.id === tabId
            ? {
                ...t,
                collectionParsed: parsed,
                collectionOriginal: parsed,
                documents: read.documents,
                parseError: read.parseError ?? null,
                ...(read.parseError && typeof read.content === 'string'
                  ? {
                      rawContent: read.content,
                      rawOriginal: read.content,
                      requestTab: 'source' as const,
                      gctfTab: 'raw' as const,
                    }
                  : {}),
              }
            : t
        );
        const updated = tabs.find(t => t.id === tabId);
        return updated && s.activeTabId === tabId ? { tabs, ...loadTab(updated) } : { tabs };
      });
    }));
  },

  saveIntent: 0,
  requestSave: () => set(s => ({ saveIntent: s.saveIntent + 1 })),
  saveAsIntent: 0,
  requestSaveAs: () => set(s => ({ saveAsIntent: s.saveAsIntent + 1 })),
  discardIntent: 0,
  requestDiscard: () => set(s => ({ discardIntent: s.discardIntent + 1 })),
  pickIntent: 0,
  requestPick: () => set(s => ({ pickIntent: s.pickIntent + 1 })),
  importIntent: 0,
  importPrefill: null,
  requestImport: (command) => set(s => ({
    importIntent: s.importIntent + 1,
    importPrefill: command ?? null,
  })),

  share: null,
  startShare: () => {
    const st = get();
    const tab = st.tabs.find(t => t.id === st.activeTabId);
    if (!tab) return 'none';
    if (tab.collectionPath) return 'link';
    const headers: Record<string, boolean> = {};
    for (const key of Object.keys(st.request.headers)) headers[key] = !isSecretHeader(key);
    set({ share: { headers, ttl: 7 } });
    return 'dialog';
  },
  closeShare: () => set({ share: null }),
  shareCreated: (link, expires) => set(s => (s.share ? { share: { ...s.share, link, expires } } : s)),
  toggleShareHeader: (key) => set(s => (
    s.share ? { share: { ...s.share, headers: { ...s.share.headers, [key]: !s.share.headers[key] } } } : s
  )),
  setShareTtl: (ttl) => set(s => (s.share ? { share: { ...s.share, ttl } } : s)),
  docsOpen: false,
  setDocsOpen: (v) => set({ docsOpen: v }),

  editChain: async (op, index = 0) => {
    const st = get();
    const path = st.workspacePath;
    const tabId = st.activeTabId;
    if (!path) return 'Save this as a file first — a chain lives in one';
    const rawWhy = rawAuthorityRefusal(rawAuthorityReason(st));
    if (rawWhy) return rawWhy;
    if (isRequestDirty(st)) return 'Save or discard this step first';
    try {
      const res = await fetch('/api/chain', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path, op, index, version: fileVersions.get(path) }),
      });
      if (!res.ok) return (await res.text()).trim() || `The server refused: ${res.status}`;
      rememberVersion(path, await res.json().catch(() => undefined));
    } catch (e: any) {
      return e?.message || 'The server could not be reached';
    }
    const read = await fetchCollection(path);
    if (!read) return 'The file could not be read back';
    const p = read.parsed;
    const step = op === 'delete' ? Math.max(0, Math.min(index, read.documents.length - 1)) : read.documents.length - 1;
    writeToTab(
      set,
      tabId,
      { collectionParsed: p, collectionOriginal: p, documents: read.documents,
        endpoint: p.endpoint, headers: p.headers, bodies: p.bodies,
        rawContent: null, rawOriginal: null },
      { documents: read.documents, collectionParsed: p, workspaceOriginal: p,
        rawContent: null, rawOriginal: null, activeStep: 0, datasetRow: 0,
        request: { endpoint: p.endpoint, headers: p.headers, bodies: p.bodies } },
    );
    set(s => withoutVerdict(s, path));
    get().selectStep(step);
    get().refreshCollections();
    return null;
  },

  retargetPath: (from, to) => set(s => {
    const tabs = s.tabs.map(t => {
      const moved = t.collectionPath ? movedPath(t.collectionPath, from, to) : null;
      return moved ? { ...t, collectionPath: moved, label: labelFor(moved) } : t;
    });
    const workspacePath = s.workspacePath ? movedPath(s.workspacePath, from, to) ?? s.workspacePath : s.workspacePath;
    const selectedCollection = s.selectedCollection
      ? movedPath(s.selectedCollection, from, to) ?? s.selectedCollection
      : s.selectedCollection;
    const verdicts = Object.fromEntries(
      Object.entries(s.run.verdicts).map(([path, verdict]) => {
        const moved = movedPath(path, from, to);
        return moved ? [moved, { ...verdict, path: moved }] : [path, verdict];
      }),
    );
    const cases = Object.fromEntries(
      Object.entries(s.run.cases).map(([id, verdict]) => {
        const moved = movedPath(fileOfCase(id), from, to);
        if (!moved) return [id, verdict];
        const renamed = moved + id.slice(fileOfCase(id).length);
        return [renamed, { ...verdict, path: renamed }];
      }),
    );
    const runData = s.runData ? movedPath(s.runData, from, to) ?? s.runData : s.runData;
    const checked = checkedAfterMove(s.checked, from, to);
    if (runData !== s.runData) writeJson('play.run.data', { path: runData, columns: s.runDataColumns });
    return { tabs, workspacePath, selectedCollection, runData, checked, run: { ...s.run, verdicts, cases } };
  }),
  closeIntent: 0,
  requestCloseTab: () => set(s => ({ closeIntent: s.closeIntent + 1 })),
  closeAllIntent: 0,
  requestCloseAllTabs: () => set(s => ({ closeAllIntent: s.closeAllIntent + 1 })),
  tabListIntent: 0,
  requestTabList: () => set(s => ({ tabListIntent: s.tabListIntent + 1 })),

  openDroppedFile: (name, content) => {
    const tabId = get().addTab({ label: name });
    set({ rawContent: content, rawOriginal: null });
    const tabs = get().tabs.map(t =>
      t.id === tabId ? { ...t, rawContent: content, rawOriginal: null } : t);
    set({ tabs, requestTab: 'source' });
  },

  saveWorkspace: async () => {
    const st = get();
    if (!st.workspacePath) return false;
    if (rawIsAuthoritative(st)) return get().saveRawContent();
    const finalName = withFamilyExt(st.workspacePath);
    const res = await postWrite('/api/save-structured', {
        ...structuredSave(st),
        path: finalName,
        original_path: st.workspacePath,
        document_index: st.activeStep,
        version: fileVersions.get(finalName),
      });
    if (!res.ok) {
      const err = await saveFailure(finalName, res);
      if (err instanceof SaveConflict) {
        set({ saveConflict: conflictOf(err, await previewOfSave(st, finalName), false) });
        markStale(set, finalName);
        return false;
      }
      throw err;
    }
    rememberVersion(finalName, await res.json().catch(() => undefined));
    set(s => withoutVerdict(s, finalName));
    clearStale(set, finalName);
    void get().recheck(finalName);
    const updatedParsed = { ...st.collectionParsed! };
    updatedParsed.endpoint = st.request.endpoint;
    updatedParsed.headers = st.request.headers;
    updatedParsed.bodies = st.request.bodies;
    writeToTab(
      set,
      st.activeTabId,
      { collectionOriginal: updatedParsed as any, collectionParsed: updatedParsed as any },
      { workspaceOriginal: updatedParsed as any, collectionParsed: updatedParsed as any },
    );
    if (st.rawContent !== null) await refreshRawFromDisk(set, st.activeTabId, finalName);
    get().refreshCollections();
    return true;
  },

  previewSave: async (path, meta, fmt) => {
    const st = get();
    const finalName = withFamilyExt(path, saveExtFor(st.workspacePath));
    if (rawIsAuthoritative(st)) {
      const raw = fmt ? await formatted(st.rawContent!, finalName) : st.rawContent!;
      let current: string | null = null;
      try {
        const read = await fetch(`/api/collections/${apiPath(finalName)}`);
        if (read.ok) {
          const data = await read.json();
          rememberVersion(finalName, data.version);
          current = typeof data.content === 'string' ? data.content : null;
        }
      } catch { /* nothing at that path yet, which the dialog reads as a new file */ }
      return { content: raw, current };
    }
    try {
      const res = await fetch('/api/preview-structured', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          ...structuredSave(st),
          path: finalName,
          meta: cleanMeta(meta),
          original_path: originalFor(st, finalName),
          fmt,
        }),
      });
      if (!res.ok) {
        const said = (await res.text().catch(() => '')).trim();
        return { error: said || `The file could not be rendered (${res.status})` };
      }
      const data = await res.json();
      rememberVersion(finalName, data.version);
      return { content: data.content ?? '', current: data.current ?? null };
    } catch {
      return { error: 'The workbench could not be reached' };
    }
  },

  saveWorkspaceAs: async (name: string, meta?: GctfMeta, fmt?: boolean) => {
    const st = get();
    const tabId = st.activeTabId;
    const finalName = withFamilyExt(name, saveExtFor(st.workspacePath));
    if (rawIsAuthoritative(st)) {
      return get().saveRawAs(finalName, fmt);
    }
    const res = await postWrite('/api/save-structured', {
        ...structuredSave(st),
        path: finalName,
        meta: cleanMeta(meta),
        original_path: originalFor(st, finalName),
        fmt,
        version: fileVersions.get(finalName),
      });
    if (!res.ok) throw await saveFailure(finalName, res);
    rememberVersion(finalName, await res.json().catch(() => undefined));
    set(s => withoutVerdict(s, finalName));
    const label = finalName.split('/').pop() || finalName;
    const updatedParsed: CollectionParsed = st.collectionParsed
      ? { ...st.collectionParsed, endpoint: st.request.endpoint, headers: st.request.headers, bodies: st.request.bodies, address: st.address }
      : {
          endpoint: st.request.endpoint, address: st.address, headers: st.request.headers, bodies: st.request.bodies,
          asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
          tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
          expect_responses: [], expect_error: null,
        };
    writeToTab(
      set,
      tabId,
      { collectionPath: finalName, label, collectionOriginal: updatedParsed, collectionParsed: updatedParsed },
      { workspacePath: finalName, selectedCollection: finalName, workspaceOriginal: updatedParsed, collectionParsed: updatedParsed },
    );
    if (st.rawContent !== null) await refreshRawFromDisk(set, tabId, finalName);
    saveTabsToStorage(get().tabs, get().activeTabId);
    get().refreshCollections();
  },

  scaffoldTest: async (endpoint) => {
    const st = get();
    const target = endpoint ?? st.request.endpoint;
    if (!target) throw new Error('Choose a method first');
    const res = await askServer(
      '/api/scaffold',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(schemaRequest(st, target)),
      },
      'nothing was scaffolded',
    );
    const data = await res.json().catch(() => null);
    if (!res.ok || !data || data.error || !data.content) {
      throw new Error(data?.error || `Server returned ${res.status}`);
    }
    const method = target.split('/').pop() || 'scaffold';
    const opened = get().addTab({
      label: `${method}.gctf`,
      endpoint: target,
      rawContent: data.content,
      rawOriginal: null,
      requestTab: 'source',
    });
    if (!opened) throw new Error('Every open tab has unsaved edits — close one first');
  },

  getCurlCommand: () => {
    const st = get();
    const env = effectiveEnvironment(
      st.activeEnvironment ? st.environments.find(e => e.name === st.activeEnvironment) ?? null : null,
    );
    const bound = boundByRun(st);
    const filled = exported(applyEnvironment(bound.endpoint, bound.headers, bound.bodies, env), st);
    const { method, path } = splitEndpoint(filled.endpoint);
    const url = httpUrl(
      resolveProjectAddress(substituteEnv(callAddress(st), env) || callAddress(st), projectCallEnv(st)),
      path,
    );
    return toCurl({
      method,
      url,
      headers: filled.headers,
      body: filled.bodies.find(b => b.trim()) ?? '',
    });
  },

  getGrpcurlCommand: async () => {
    const st = get();
    const { protocol } = st;
    const env = effectiveEnvironment(
      st.activeEnvironment ? st.environments.find(e => e.name === st.activeEnvironment) ?? null : null,
    );
    const bound = boundByRun(st);
    const request = exported(
      applyEnvironment(bound.endpoint, bound.headers, bound.bodies, env),
      st,
    );
    const address = resolveProjectAddress(
      substituteEnv(callAddress(st), env) || callAddress(st),
      projectCallEnv(st),
    );
    const { tls, tlsInsecure } = effectiveTls(st);
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
      address,
      tls: tls || undefined,
      tls_insecure: tls ? tlsInsecure : undefined,
      protocol: protocol || undefined,
      collection_path: st.workspacePath ?? undefined,
    };
    const metaJson = JSON.stringify(meta);
    const payload = metaJson === '{}'
      ? `{"body":${bodyLiteral}}`
      : `${metaJson.slice(0, -1)},"body":${bodyLiteral}}`;
    const res = await askServer(
      '/api/grpcurl',
      { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: payload },
      'the command was not built',
    );
    if (!res.ok) {
      const said = await res.text().catch(() => '');
      throw new Error(said.trim() || 'The command could not be built');
    }
    const data = await res.json();
    if (data.command?.startsWith('# error')) throw new Error(data.command);
    return data.command || 'grpcurl ...';
  },

  execute: async () => {
    const st = get();
    const { workspacePath, address, protocol } = st;
    const { tls, tlsInsecure, tlsCa, tlsCert, tlsKey } = effectiveTls(st);

    const tabId = st.activeTabId;

    const fromStep = st.activeStep;
    const writeResponse = (result: CallResult | null, extra?: Partial<PlayStore>) => set(s => {
      const r = result && { ...result, fromStep };
      const tabs = s.tabs.map(t => t.id === tabId ? { ...t, response: r } : t);
      const patch: Partial<PlayStore> = { tabs, ...extra };
      if (s.activeTabId === tabId) patch.response = r;
      return patch;
    });

    const activeEnv = st.activeEnvironment
      ? st.environments.find(e => e.name === st.activeEnvironment)
      : null;

    if (!st.request.endpoint) {
      const want = isHttpRequest(st.workspacePath, st.request.endpoint)
        ? 'Enter a method and a path'
        : 'Enter a gRPC endpoint';
      const errResult: CallResult = { status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: want, durationMs: null, sent: false };
      writeResponse(errResult);
      return;
    }

    const effectiveEnv = effectiveEnvironment(activeEnv);

    const runBound = bindingsOf(st);
    const fromRun = applyBindings(st.request.endpoint, st.request.headers, st.request.bodies, runBound);
    const substituted = applyEnvironment(fromRun.endpoint, fromRun.headers, fromRun.bodies, effectiveEnv);
    const resolvedVars = answeredHere(st);

    const resolved = activeEnv ? substituteEnv(address, effectiveEnv) || address : address;
    const dialled = resolveProjectAddress(
      st.collectionParsed?.address?.trim()
        || chainAddressAt(st.documents, st.activeStep)
        || dialledAddress(resolved, protocol, st.serverEnv.address, activeEnvAddress(st)),
      projectCallEnv(st),
    );
    const used = connectionUsed(st.collectionParsed, { protocol, tls, tlsInsecure });
    if (address.trim()) get().rememberAddress(address.trim());
    set({ lastCallAddress: dialled });

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
          headers: namedHeaders(substituted.headers),
          tls: tls || undefined, tls_insecure: tls ? tlsInsecure : undefined,
          tls_ca: tls ? (tlsCa || undefined) : undefined,
          tls_cert: tls ? (tlsCert || undefined) : undefined,
          tls_key: tls ? (tlsKey || undefined) : undefined,
          address: dialled,
          protocol: protocol || undefined,
          collection_path: workspacePath || undefined,
          document_index: st.activeStep,
          dataset_row: (st.collectionParsed?.dataset?.length ?? 0) > 0
            ? clampRow(st.collectionParsed?.dataset, st.datasetRow)
            : undefined,
          session_id: st.sessionId || undefined,
          timeout_seconds: st.requestTimeoutMs > 0 ? Math.max(1, Math.ceil(st.requestTimeoutMs / 1000)) : undefined,
        }),
        signal,
      });
      clearController();

      const said = await res.text().catch(() => '');
      if (!res.ok) {
        writeResponse({
          status: 'error', statusCode: null, messages: [], headers: {}, trailers: {},
          error: said.trim() || `The workbench refused this request (${res.status} ${res.statusText})`,
          durationMs: null,
          sent: false,
        });
        return;
      }
      let data: any;
      try { data = JSON.parse(said); } catch {
        const errResult: CallResult = { status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: said.trim() || `Server returned ${res.status} ${res.statusText}`, durationMs: Math.round(performance.now() - start) };
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
        statusCode: data.grpc_status ?? null,
        shape: data.shape ?? null,
        messages: data.messages ?? [],
        messageOffsetsMs: data.message_offsets_ms ?? [],
        headers: data.headers || {},
        trailers: data.trailers || {},
        error: data.error || null,
        durationMs,
        messagesRaw: data.messages_raw ?? [],
        messagesTotal: data.messages_total ?? (data.messages ?? []).length,
        messagesTruncated: data.messages_truncated ?? false,
      };
      const boundNow = Array.isArray(data.extracted) ? (data.extracted as [string, string][]) : [];
      if (workspacePath && boundNow.length > 0) {
        set(s => {
          const held = new Map(s.executeBound[workspacePath] ?? []);
          for (const [name, value] of boundNow) held.set(name, value);
          return { executeBound: { ...s.executeBound, [workspacePath]: [...held] } };
        });
      }

      const entry: HistoryEntry = {
        id: id(), timestamp: now(), endpoint: st.request.endpoint,
        bodies: st.request.bodies, headers: st.request.headers, response: result,
        ...(resolvedVars.length > 0 ? { resolved: resolvedVars } : {}),
        ...(workspacePath ? { collectionPath: workspacePath } : {}),
        ...((st.collectionParsed?.dataset?.length ?? 0) > 0
          ? { datasetRow: clampRow(st.collectionParsed?.dataset, st.datasetRow) }
          : {}),
        connection: {
          address: dialled,
          ...(isHttpRequest(st.workspacePath, st.request.endpoint)
            ? {}
            : { protocol: used.protocol }),
          tls: used.tls,
          ...(used.tls ? { tlsInsecure: used.tlsInsecure } : {}),
        },
      };
      historyCache.put(entry.id, entry);
      saveHistoryToStorage();
      if (!callFailed(result, isHttpRequest(st.workspacePath, st.request.endpoint))) {
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

  runTest: async () => {
    const st = get();
    const { workspacePath, sessionId } = st;
    const tabId = st.activeTabId;
    let fromStep: number | undefined = st.documents.length > 1 ? undefined : st.activeStep;
    const writeResponse = (result: CallResult | null, extra?: Partial<PlayStore>) => set(s => {
      const r = result && (fromStep === undefined ? result : { ...result, fromStep });
      const tabs = s.tabs.map(t => t.id === tabId ? { ...t, response: r } : t);
      const patch: Partial<PlayStore> = { tabs, ...extra };
      if (s.activeTabId === tabId) patch.response = r;
      return patch;
    });

    if (!workspacePath) {
      writeResponse({ status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: `Save this as a file before running it — a run executes the saved ${isHttpRequest(null, st.request.endpoint) ? '.httf' : '.gctf'}, not the unsaved editor state.`, durationMs: null, sent: false });
      return;
    }

    set({ runStatus: 'running' });
    const pending: CallResult = { status: 'pending', statusCode: null, messages: [], headers: {}, trailers: {}, error: null, durationMs: null };
    writeResponse(pending);
    setVerdict(set, workspacePath, { path: workspacePath, state: 'running' });
    const start = performance.now();
    try {
      const res = await fetch('/api/run', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          collection_path: workspacePath,
          session_id: sessionId || undefined,
          data: get().runData || undefined,
        }),
      });
      const durationMs = Math.round(performance.now() - start);
      const said = await res.text().catch(() => '');
      if (res.status === 404) {
        writeResponse({
          status: 'error', statusCode: null, messages: [], headers: {}, trailers: {},
          error: `${workspacePath} is not on disk any more — Save writes this tab back to it`,
          durationMs,
        });
        return;
      }
      let data: any;
      try { data = JSON.parse(said); } catch {
        writeResponse({ status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: said.trim() || `Server returned ${res.status} ${res.statusText}`, durationMs });
        return;
      }
      if (!res.ok) {
        writeResponse({ status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: typeof data === 'string' ? data : (data.error || `Server returned ${res.status}`), durationMs });
        return;
      }
      const result: CallResult = {
        status: data.success ? 'ok' : 'error',
        statusCode: data.grpc_status ?? null,
        messages: data.response_messages ?? [],
        headers: data.headers || {},
        trailers: data.trailers || {},
        error: data.error || null,
        durationMs: data.call_duration_ms ?? durationMs,
        assertions: data.assertions ?? [],
        fromCase: typeof data.row === 'number'
          ? caseTitle(`row=${data.row}`, typeof data.rows_total === 'number' ? data.rows_total : undefined) ?? undefined
          : undefined,
      };
      if (typeof data.address === 'string' && data.address.trim() !== '') {
        set({ lastCallAddress: data.address });
      }
      const ran: number[] = data.documents ?? [];
      if (ran.length > 0) fromStep = ran.length - 1;
      setVerdict(set, workspacePath, {
        path: workspacePath,
        state: data.success ? 'pass' : 'fail',
        durationMs: result.durationMs ?? undefined,
        message: data.error || undefined,
        assertions: data.assertions ?? [],
        documents: data.documents ?? [],
        response: { messages: result.messages, headers: result.headers, trailers: result.trailers, error: result.error },
        ...(Array.isArray(data.extracted) && data.extracted.length > 0
          ? { extracted: data.extracted as [string, string][] }
          : {}),
      });
      if (data.success) {
        const totalOk = get().totalOk + 1;
        saveTotals(totalOk, get().totalError);
        writeResponse(result, { responseTab: 'assertions', totalOk });
      } else {
        const totalError = get().totalError + 1;
        saveTotals(get().totalOk, totalError);
        writeResponse(result, { responseTab: 'assertions', totalError });
      }
    } catch (err: any) {
      writeResponse({ status: 'error', statusCode: null, messages: [], headers: {}, trailers: {}, error: err?.message || String(err), durationMs: Math.round(performance.now() - start) });
    } finally {
      set({ runStatus: 'idle' });
      const stuck = get().run.verdicts[workspacePath];
      if (stuck?.state === 'running') {
        const said = get().tabs.find(t => t.id === tabId)?.response;
        setVerdict(set, workspacePath, {
          path: workspacePath,
          state: 'fail',
          message: said?.error ?? undefined,
          durationMs: said?.durationMs ?? undefined,
        });
      }
    }
  },

  discardEdits: async () => {
    const st = get();
    const path = st.workspacePath;
    if (!path || !workspaceDirty(st)) return false;
    const read = await fetchCollection(path);
    if (!read) return false;
    const step = Math.min(st.activeStep, Math.max(0, read.documents.length - 1));
    const doc = read.documents[step];
    const parsed = step === 0 || !doc ? read.parsed : parsedForStep(read.parsed, doc);
    const bodies = bodiesFor(path, parsed.endpoint, parsed.bodies);
    writeToTab(
      set,
      st.activeTabId,
      { collectionParsed: parsed, collectionOriginal: parsed, documents: read.documents,
        parseError: read.parseError ?? null, rawContent: null, rawOriginal: null,
        addressTouched: false, protocolTouched: false,
        endpoint: parsed.endpoint, headers: parsed.headers, bodies },
      { collectionParsed: parsed, workspaceOriginal: parsed, headParsed: read.parsed,
        documents: read.documents, activeStep: step,
        parseError: read.parseError ?? null, rawContent: null, rawOriginal: null,
        addressTouched: false, protocolTouched: false, address: '',
        request: { endpoint: parsed.endpoint, headers: { ...parsed.headers }, bodies } },
    );
    clearStale(set, path);
    return true;
  },

  loadRawContent: async () => {
    const st = get();
    if (!st.workspacePath) return;
    if (st.rawContent !== null) return;
    const tabId = st.activeTabId;
    set({ rawError: null });
    let res: Response;
    try {
      res = await fetch(`/api/collections/${apiPath(st.workspacePath)}`);
    } catch {
      if (get().activeTabId === tabId) {
        set({ rawError: 'The workbench could not be reached — the source is not loaded' });
      }
      return;
    }
    if (!res.ok) {
      if (get().activeTabId === tabId) {
        set({
          rawError: res.status === 404
            ? `${st.workspacePath} is not in this workbench any more`
            : `The file could not be read (${res.status})`,
        });
      }
      return;
    }
    const data = await res.json();
    rememberVersion(st.workspacePath, data.version);
    const content: string = data.content ?? '';
    writeToTab(set, tabId, { rawContent: content, rawOriginal: content }, { rawContent: content, rawOriginal: content });
  },

  resolveSaveConflict: async (choice) => {
    const c = get().saveConflict;
    set({ saveConflict: null });
    if (!c || choice === 'cancel') return;

    if (choice === 'overwrite') {
      fileVersions.delete(c.path);
      await (c.raw ? get().saveRawContent() : get().saveWorkspace());
      return;
    }

    fileVersions.delete(c.path);
    const conflicted = get().tabs.find(t => t.collectionPath === c.path)?.id ?? get().activeTabId;
    writeToTab(
      set,
      conflicted,
      { rawContent: c.theirs, rawOriginal: c.theirs },
      { rawContent: c.theirs, rawOriginal: c.theirs },
    );
    clearStale(set, c.path);
    const read = await fetchCollection(c.path);
    if (!read) return;
    const parsed = read.parsed;
    const bodies = bodiesFor(c.path, parsed.endpoint, parsed.bodies);
    writeToTab(
      set,
      conflicted,
      { endpoint: parsed.endpoint, headers: parsed.headers, bodies,
        collectionParsed: parsed, collectionOriginal: parsed },
      { collectionParsed: parsed, workspaceOriginal: parsed,
        request: { endpoint: parsed.endpoint, headers: parsed.headers, bodies } },
    );
  },

  setDatasetRow: (index) => set({ datasetRow: index }),

  setActiveStep: (index) => set({ activeStep: index }),

  selectStep: (index) => {
    const st = get();
    if (index === st.activeStep) return true;
    if (isRequestDirty(st)) return false;

    const head = st.headParsed ?? st.collectionParsed;
    const step = st.documents[index];
    if (!head || !step) {
      set({ activeStep: index });
      return true;
    }
    const parsed = index === 0 ? head : parsedForStep(head, step);
    set({
      activeStep: index,
      headParsed: head,
      collectionParsed: parsed,
      workspaceOriginal: parsed,
      request: { endpoint: parsed.endpoint, headers: { ...parsed.headers }, bodies: [...parsed.bodies] },
    });
    return true;
  },

  addAssert: (expr) => {
    const line = expr.trim();
    if (!line) return 'empty';
    const st = get();
    const p = parsedOrShell(st);
    if (p.asserts.includes(line)) return 'duplicate';
    set(withParsed(st, { ...p, asserts: [...p.asserts, line] }));
    return 'added';
  },

  removeAssert: (index) => {
    const st = get();
    const p = parsedOrShell(st);
    set(withParsed(st, { ...p, asserts: p.asserts.filter((_, i) => i !== index) }));
  },

  replaceAssert: (index, line) => {
    const st = get();
    const p = parsedOrShell(st);
    if (index < 0 || index >= p.asserts.length) return;
    set(withParsed(st, { ...p, asserts: p.asserts.map((a, i) => (i === index ? line : a)) }));
  },

  setExpectMode: (mode) => {
    const st = get();
    const p = parsedOrShell(st);
    if (mode === 'none') {
      set(withParsed(st, { ...p, expect_responses: [], expect_error: null }));
      return;
    }
    if (mode === 'response') {
      const responses = p.expect_responses.length > 0 ? p.expect_responses : [blankExpect()];
      set(withParsed(st, { ...p, expect_responses: responses, expect_error: null }));
      return;
    }
    const error = p.expect_error ?? { ...blankExpect(), body: '{}' };
    set(withParsed(st, { ...p, expect_responses: [], expect_error: error }));
  },

  setExpectResponse: (index, patch) => {
    const st = get();
    const p = parsedOrShell(st);
    if (index < 0 || index >= p.expect_responses.length) return;
    const expect_responses = p.expect_responses.map((m, i) => (i === index ? { ...m, ...patch } : m));
    set(withParsed(st, { ...p, expect_responses }));
  },

  focusAnswerStep: () => {
    const st = get();
    const from = st.response?.fromStep;
    if (from === undefined || from === st.activeStep) return true;
    return get().selectStep(from);
  },

  expectFromResponse: () => {
    const first = get();
    const answer = first.response;
    if (!answer || answer.status === 'pending') return false;
    if (!serverAnswered(answer)) return false;
    if (!get().focusAnswerStep()) return false;
    const st = get();
    const r = st.response;
    if (!r || r.status === 'pending') return false;
    const p = parsedOrShell(st);

    if (isHttpRequest(st.workspacePath, st.request.endpoint)) {
      const line = `@status() == ${r.statusCode ?? 200}`;
      const asserts = p.asserts.some(a => a.trim().startsWith('@status()'))
        ? p.asserts.map(a => (a.trim().startsWith('@status()') ? line : a))
        : [line, ...p.asserts];
      const expect_responses = r.messages.length > 0
        ? [{ ...blankExpect(), body: expectBody(r.messages[0], r.messagesRaw?.[0]) }]
        : [];
      set({
        ...withParsed(st, { ...p, asserts, expect_responses, expect_error: null }),
        requestTab: 'asserts',
      });
      return true;
    }

    const failed = callFailed(r, false);
    if (failed) {
      const body = errorExpectBody(r.statusCode ?? null, errorText(r.error ?? ''));
      set({
        ...withParsed(st, { ...p, expect_responses: [], expect_error: { ...blankExpect(), body } }),
        requestTab: 'asserts',
      });
      return true;
    }

    const expect_responses = r.messages.length === 0
      ? [{ ...blankExpect(), body: '' }]
      : r.messages.map((m, i) => ({ ...blankExpect(), body: expectBody(m, r.messagesRaw?.[i]) }));
    set({
      ...withParsed(st, { ...p, expect_responses, expect_error: null }),
      requestTab: 'asserts',
    });
    return true;
  },

  addExpectResponse: () => {
    const st = get();
    const p = parsedOrShell(st);
    set(withParsed(st, { ...p, expect_responses: [...p.expect_responses, blankExpect()], expect_error: null }));
  },

  removeExpectResponse: (index) => {
    const st = get();
    const p = parsedOrShell(st);
    set(withParsed(st, { ...p, expect_responses: p.expect_responses.filter((_, i) => i !== index) }));
  },

  setExpectError: (patch) => {
    const st = get();
    const p = parsedOrShell(st);
    if (!p.expect_error) return;
    set(withParsed(st, { ...p, expect_error: { ...p.expect_error, ...patch } }));
  },

  addExtract: (name, expr) => {
    const { name: key, kind } = splitExtractName(name);
    const value = expr.trim();
    if (!key || !value) return;
    const st = get();
    const p = parsedOrShell(st);
    set(withParsed(st, {
      ...p,
      extracts: { ...p.extracts, [key]: value },
      extract_types: kind
        ? { ...(p.extract_types ?? {}), [key]: kind }
        : p.extract_types,
    }));
  },

  setSectionKv: (section, kv) => {
    const st = get();
    set(withParsed(st, { ...parsedOrShell(st), [section]: kv }));
  },

  setMetaField: (field, value) => {
    const st = get();
    set(withParsed(st, { ...parsedOrShell(st), [field]: value.trim() ? value : null }));
  },

  setMetaTags: (tags) => {
    const st = get();
    set(withParsed(st, { ...parsedOrShell(st), meta_tags: tags }));
  },

  setMetaLinks: (links) => {
    const st = get();
    set(withParsed(st, { ...parsedOrShell(st), meta_links: links.map(l => l.trim()).filter(Boolean) }));
  },

  setDataset: (rows) => {
    const st = get();
    set(withParsed(st, { ...parsedOrShell(st), dataset: rows }));
  },

  renameExtractVariable: async (from, to, options) => {
    const st = get();
    const path = st.workspacePath;
    if (!path) return { refused: 'Save this as a file first — a chain lives in one' };
    const rawWhy = rawAuthorityRefusal(rawAuthorityReason(st));
    if (rawWhy) return { refused: rawWhy };
    if (isRequestDirty(st)) return { refused: 'Save or discard this step first' };
    let res: Response;
    try {
      res = await fetch('/api/rename-variable', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path, from, to, dataset: options?.dataset ?? false }),
      });
    } catch {
      return { refused: 'The workbench could not be reached' };
    }
    if (!res.ok) return { refused: (await res.text()).trim() || `The server refused: ${res.status}` };
    const data = await res.json().catch(() => ({ rewritten: 0 }));
    const read = await fetchCollection(path);
    if (read) {
      writeToTab(
        set,
        st.activeTabId,
        { collectionParsed: read.parsed, collectionOriginal: read.parsed, documents: read.documents,
          endpoint: read.parsed.endpoint, headers: read.parsed.headers,
          bodies: bodiesFor(path, read.parsed.endpoint, read.parsed.bodies) },
        { collectionParsed: read.parsed, workspaceOriginal: read.parsed, documents: read.documents,
          request: { endpoint: read.parsed.endpoint, headers: { ...read.parsed.headers },
            bodies: bodiesFor(path, read.parsed.endpoint, read.parsed.bodies) } },
      );
    }
    return { rewritten: data.rewritten ?? 0 };
  },

  renameDatasetColumn: (from, to) => {
    const st = get();
    const name = to.trim();
    if (!name || name === from) return 0;
    const p = parsedOrShell(st);
    const request = st.request;
    const touched = countDatasetRefs(
      [request.endpoint, ...Object.values(request.headers), ...request.bodies],
      from,
    );
    const next = {
      endpoint: renameDatasetRefs(request.endpoint, from, name),
      headers: Object.fromEntries(
        Object.entries(request.headers).map(([k, v]) => [k, renameDatasetRefs(v, from, name)]),
      ),
      bodies: request.bodies.map(b => renameDatasetRefs(b, from, name)),
    };
    set({
      ...withParsed(st, { ...p, dataset: renameColumn(p.dataset ?? [], from, name), ...next }),
      request: next,
    });
    return touched;
  },

  removeExtract: (name) => {
    const st = get();
    const p = parsedOrShell(st);
    const extracts = { ...p.extracts };
    delete extracts[name];
    set(withParsed(st, { ...p, extracts }));
  },

  setRawContent: (v) => set(s => {
    const tabs = s.tabs.map(t => t.id === s.activeTabId ? { ...t, rawContent: v } : t);
    return { tabs, rawContent: v };
  }),

  saveRawAs: async (path: string, fmt?: boolean) => {
    const st = get();
    if (st.rawContent === null) return;
    const content = fmt ? await formatted(st.rawContent, path) : st.rawContent;
    const tabId = st.activeTabId;
    const res = await postWrite('/api/save', {
        path,
        content,
        version: fileVersions.get(path),
        original_path: st.workspacePath ?? undefined,
      });
    if (!res.ok) throw await saveFailure(path, res);
    rememberVersion(path, await res.json().catch(() => undefined));
    const label = path.split('/').pop() || path;
    const saved = content;
    writeToTab(
      set,
      tabId,
      { collectionPath: path, label, rawContent: saved, rawOriginal: saved },
      { workspacePath: path, selectedCollection: path, rawContent: saved, rawOriginal: saved },
    );
    saveTabsToStorage(get().tabs, get().activeTabId);
    const read = await fetchCollection(path);
    if (read) {
      const bodies = bodiesFor(path, read.parsed.endpoint, read.parsed.bodies);
      writeToTab(
        set,
        tabId,
        { collectionParsed: read.parsed, collectionOriginal: read.parsed, documents: read.documents,
          parseError: read.parseError ?? null,
          endpoint: read.parsed.endpoint, headers: read.parsed.headers, bodies },
        { collectionParsed: read.parsed, workspaceOriginal: read.parsed,
          parseError: read.parseError ?? null, documents: read.documents,
          request: { endpoint: read.parsed.endpoint, headers: { ...read.parsed.headers }, bodies } },
      );
    }
    get().refreshCollections();
  },

  saveRawContent: async () => {
    const st = get();
    if (!st.workspacePath || st.rawContent === null) return false;
    const res = await postWrite('/api/save', {
        path: st.workspacePath,
        content: st.rawContent,
        version: fileVersions.get(st.workspacePath),
      });
    if (!res.ok) {
      const err = await saveFailure(st.workspacePath, res);
      if (err instanceof SaveConflict) {
        set({ saveConflict: conflictOf(err, st.rawContent, true) });
        markStale(set, st.workspacePath);
        return false;
      }
      throw err;
    }
    rememberVersion(st.workspacePath, await res.json().catch(() => undefined));
    set(s => withoutVerdict(s, st.workspacePath!));
    clearStale(set, st.workspacePath);
    void get().recheck(st.workspacePath);
    const savedContent = st.rawContent;
    writeToTab(set, st.activeTabId, { rawOriginal: savedContent }, { rawOriginal: savedContent });
    const res2 = await fetch(`/api/collections/${apiPath(st.workspacePath)}`);
    const data = res2.ok ? await res2.json().catch(() => null) : null;
    if (data?.parsed) {
      rememberVersion(st.workspacePath, data.version);
      const p: CollectionParsed = data.parsed;
      const bodies = bodiesFor(st.workspacePath, p.endpoint, p.bodies);
      writeToTab(
        set,
        st.activeTabId,
        {
          endpoint: p.endpoint, headers: p.headers, bodies,
          collectionParsed: p, collectionOriginal: p,
          parseError: data.parse_error ?? null,
        },
        {
          collectionParsed: p, workspaceOriginal: p,
          parseError: data.parse_error ?? null,
          request: { endpoint: p.endpoint, headers: p.headers, bodies },
        },
      );
    }
    get().refreshCollections();
    return true;
  },

  loadStartupInfo: async () => {
    try {
      const res = await fetch('/api/info');
      if (!res.ok) { set({ serverHealthy: false }); return null; }
      const data = await res.json();
      void get().refreshChanged();

      const root: string = data.root || '';
      tabsRoot = root;
      let foreign: string | null = null;
      if (root && storedTabsRoot && storedTabsRoot !== root) {
        const before = get().tabs;
        const kept = keepFromAnotherRoot(before);
        const dropped = before.length - kept.length;
        if (dropped > 0) {
          const tabs = kept.length > 0 ? kept : [defaultTab()];
          const activeTabId = tabs.some(t => t.id === get().activeTabId) ? get().activeTabId : tabs[0].id;
          saveTabsToStorage(tabs, activeTabId);
          set({ tabs, activeTabId, ...loadTab(tabs.find(t => t.id === activeTabId) ?? tabs[0]) });
          foreign = `${count(dropped, 'tab')} left over from ${storedTabsRoot} — closed, because this workbench serves ${root}`;
          set({ startupNote: foreign });
        }
      }
      storedTabsRoot = root;

      set({
        version: data.version || '',
        workspaceName: data.workspace || '',
        sharesPath: data.shares_path || '.grpctestify/shares',
        serverHealthy: data.status === 'ok',
        collectionsMtime: data.collections_mtime ?? 0,
        serverEnv: data.env ?? {},
      });

      if (data.project?.active) {
        set({
          projectRoot: data.project.project_dir || '.grpctestify',
          projectRootAbs: data.project.project_dir_abs ?? null,
          collectionsDir: data.project.collections_dir ?? null,
          projectEnvNames: data.project.envs || [],
        });
        await initProjectEnvs(data.project.envs || []);
        const sdata = await fetch('/api/project/settings').then(r => r.ok ? r.json() : null).catch(() => null);
        if (sdata) {
          const st = get();
          set({
            projectDefaults: {
              address: sdata.address ?? '',
              protocol: (sdata.protocol ?? 'grpc') as WireProtocol,
              tls: sdata.tls ?? false,
              tlsInsecure: sdata.tls_insecure ?? true,
              activeEnv: sdata.active_env ?? null,
            },
          });
          const chosen = st.addressTouched || st.protocolTouched;
          if (!chosen) {
            set({
              address: sdata.address || st.address,
              protocol: sdata.protocol || st.protocol,
              tls: sdata.tls ?? st.tls,
              tlsInsecure: sdata.tls_insecure ?? st.tlsInsecure,
            });
          }
          if (sdata.active_env && get().environments.some(e => e.name === sdata.active_env)) {
            set({ activeEnvironment: sdata.active_env });
          }
        }
      }
      return foreign;
    } catch {
      set({ serverHealthy: false });
      return null;
    }
  },

  checkHealth: async () => {
    try {
      const res = await fetch('/api/health');
      if (!res.ok) { set({ serverHealthy: false }); return; }
      const data = await res.json().catch(() => null);
      set({ serverHealthy: true, buildMoved: buildMoved(loadedBuild(), data?.build) });
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
      const browser = [...s.browserEnvs.filter(e => e.name !== env.name), { ...env, source: 'browser' as const }];
      saveBrowserEnvs(browser);
      return { browserEnvs: browser, ...merged(s.projectEnvs, browser) };
    });
  },
  updateEnvironment: (name, env) => {
    set(s => {
      const browser = s.browserEnvs.map(e => e.name === name ? { ...env, source: 'browser' as const } : e);
      const activeEnvironment = s.activeEnvironment === name ? env.name : s.activeEnvironment;
      saveBrowserEnvs(browser);
      try { if (activeEnvironment) localStorage.setItem(ACTIVE_ENV_KEY, activeEnvironment); } catch {  }
      return { browserEnvs: browser, ...merged(s.projectEnvs, browser), activeEnvironment };
    });
  },
  deleteEnvironment: (name) => {
    set(s => {
      const browser = s.browserEnvs.filter(e => e.name !== name);
      saveBrowserEnvs(browser);
      return {
        browserEnvs: browser,
        ...merged(s.projectEnvs, browser),
        activeEnvironment: s.activeEnvironment === name ? null : s.activeEnvironment,
      };
    });
  },

  pinTab: (tabId) => {
    set(state => {
      const tabs = state.tabs.map(t => (t.id === tabId ? { ...t, isPreview: false } : t));
      saveTabsToStorage(tabs, state.activeTabId);
      return { tabs };
    });
  },

  restoreHistory: (entry, options) => {
    const state = get();
    const pin = options?.pin ?? false;

    if (entry.kind === 'run' && entry.collectionPath) {
      void get().loadCollection(entry.collectionPath, { pin: true });
      return;
    }

    if (entry.datasetRow !== undefined) set({ datasetRow: entry.datasetRow });

    if (entry.connection) {
      const { address, protocol, tls, tlsInsecure } = entry.connection;
      set({
        address,
        tls,
        ...(protocol === undefined ? {} : { protocol }),
        ...(tlsInsecure === undefined ? {} : { tlsInsecure }),
      });
      saveSettings(clientSettings(get()));
    }

    const already = state.tabs.find(t => t.originHistoryId === entry.id)
      ?? tabHoldingCall(state.tabs, entry);
    if (already) {
      const tabs = pin && already.isPreview
        ? state.tabs.map(t => (t.id === already.id ? { ...t, isPreview: false } : t))
        : state.tabs;
      saveTabsToStorage(tabs, already.id);
      const focused = tabs.find(t => t.id === already.id)!;
      set({ tabs, activeTabId: already.id, ...loadTab(focused) });
      return;
    }

    const restored: Tab = {
      ...defaultTab(),
      id: id(),
      label: suggestedFileName(entry.endpoint) || entry.endpoint || 'History',
      endpoint: entry.endpoint,
      headers: entry.headers,
      bodies: entry.bodies,
      response: entry.response,
      originHistoryId: entry.id,
      isPreview: !pin,
    };

    const previewIdx = previewSlot(state.tabs);
    const tabs = previewIdx >= 0 && !pin
      ? state.tabs.map((t, i) => (i === previewIdx ? restored : t))
      : [...state.tabs, restored];

    saveTabsToStorage(tabs, restored.id);
    set({ tabs, activeTabId: restored.id, ...loadTab(restored) });
  },

  setHistory: (v) => set({ history: v }),
  forgetHistory: (id) => {
    historyCache.delete(id);
    const history = get().history.filter(e => e.id !== id);
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(history)); } catch { /* private mode */ }
    set({ history });
  },

  clearHistory: () => {
    historyCache.clear();
    try { localStorage.removeItem(STORAGE_KEY); } catch {  }
    set({ history: [] });
  },

  saveProjectSettings: async (s) => {
    try {
      const res = await fetch('/api/project/settings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(s),
      });
      if (res.ok) {
        set(state => ({
          projectDefaults: {
            address: s.address ?? state.projectDefaults?.address ?? '',
            protocol: (s.protocol ?? state.projectDefaults?.protocol ?? 'grpc') as WireProtocol,
            tls: s.tls ?? state.projectDefaults?.tls ?? false,
            tlsInsecure: s.tls_insecure ?? state.projectDefaults?.tlsInsecure ?? true,
            activeEnv: s.active_env ?? null,
          },
        }));
      }
      return res.ok;
    } catch {
      return false;
    }
  },

  fetchVariableUses: async () => {
    const res = await fetch('/api/variables');
    if (!res.ok) return [];
    return res.json();
  },

  refreshProjectEnvs: async () => {
    const names = get().projectEnvNames;
    if (names.length > 0) await initProjectEnvs(names);
  },

  fetchProjectEnv: async (name) => {
    let res: Response;
    try {
      res = await fetch(`/api/project/env/${encodeURIComponent(name)}`);
    } catch {
      throw new Error('The workbench could not be reached — the environment is not loaded');
    }
    if (!res.ok) {
      const said = await res.text().catch(() => '');
      throw new Error(said.trim() || `.env.${name} could not be read`);
    }
    return res.json();
  },

  saveProjectEnv: async (name, content) => {
    await writeOrSay(
      `/api/project/env/${encodeURIComponent(name)}`,
      { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ content }) },
      `.env.${name} could not be written`,
    );
  },

  fetchProjectEnvLocal: async (name) => {
    const res = await fetch(`/api/project/env/${encodeURIComponent(name)}/local`);
    if (!res.ok) return { exists: false, content: null };
    return res.json();
  },

  saveProjectEnvLocal: async (name, content) => {
    await writeOrSay(
      `/api/project/env/${encodeURIComponent(name)}/local`,
      { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ content }) },
      `.env.${name}.local could not be written`,
    );
  },

  deleteProjectEnv: async (name) => {
    await writeOrSay(
      `/api/project/env/${encodeURIComponent(name)}`,
      { method: 'DELETE' },
      `.env.${name} could not be deleted`,
    );
    set(s => ({ projectEnvNames: s.projectEnvNames.filter(n => n !== name) }));
    await get().refreshProjectEnvs();
    if (get().activeEnvironment === name) get().setActiveEnvironment(null);
  },

  deleteProjectEnvLocal: async (name) => {
    await writeOrSay(
      `/api/project/env/${encodeURIComponent(name)}/local`,
      { method: 'DELETE' },
      `.env.${name}.local could not be deleted`,
    );
  },

  duplicateCollection: async (path) => {
    const read = await fetch(`/api/collections/${apiPath(path)}`).catch(() => null);
    const data = read?.ok ? await read.json().catch(() => null) : null;
    if (typeof data?.content !== 'string') {
      throw new Error(`${path} could not be read — nothing was copied`);
    }
    const name = nextCopyName(path, listedPaths(get()) ?? []);
    const res = await postWrite('/api/save', { path: name, content: data.content });
    if (!res.ok) {
      const said = await res.text().catch(() => '');
      throw new Error(said.trim() || `${name} could not be written`);
    }
    rememberVersion(name, await res.json().catch(() => undefined));
    await get().refreshCollections();
    await get().loadCollection(name, { pin: true });
    return name;
  },

  formatFile: async () => {
    const st = get();
    if (!st.workspacePath) throw new Error('Open a file to format it');
    if (formsAheadOfFile(st)) {
      throw new Error('The forms hold edits the formatter would drop — save them first');
    }
    if (st.rawContent === null) await get().loadRawContent();
    const text = get().rawContent;
    if (text === null) throw new Error(get().rawError ?? 'The file could not be read');
    const next = await formatted(text, st.workspacePath);
    if (next === text) return 0;
    get().setRawContent(next);
    return lineDiff(text, next).filter(l => l.kind !== 'same').length;
  },

  toggleSidebar: () => set(s => ({ sidebarVisible: !s.sidebarVisible })),
  setShowHotkeyHelp: (v) => set({ showHotkeyHelp: v }),

  syncOpenFiles: async () => {
    const st = get();
    const open = [...new Set(st.tabs.map(t => t.collectionPath).filter((p): p is string => !!p))];
    const onDisk = open.length === 0 ? new Map<string, FileVersion>() : await diskVersions(open);
    const changed: string[] = [];
    const stale: string[] = [];
    for (const tab of st.tabs) {
      const path = tab.collectionPath;
      if (!path) continue;
      const seen = fileVersions.get(path);
      const now = onDisk.get(path);
      if (seen === undefined || now === undefined || now.hash === seen.hash) continue;
      if (isTabDirty(tab)) { stale.push(path); continue; }
      const read = await fetchCollection(path);
      if (!read) continue;
      changed.push(path);
      void get().recheck(path);
      set(s => {
        const cleared = withoutVerdict(s, path);
        const tabs = s.tabs.map(t => t.id !== tab.id ? t : {
          ...t,
          collectionParsed: read.parsed,
          collectionOriginal: read.parsed,
          documents: read.documents,
          parseError: read.parseError ?? null,
          endpoint: read.parsed.endpoint,
          headers: read.parsed.headers,
          bodies: bodiesFor(path, read.parsed.endpoint, read.parsed.bodies),
          ...(t.rawContent !== null && typeof read.content === 'string'
            ? { rawContent: read.content, rawOriginal: read.content }
            : {}),
        });
        const active = tabs.find(t => t.id === s.activeTabId);
        return active && active.id === tab.id
          ? { ...cleared, tabs, ...loadTab(active) }
          : { ...cleared, tabs };
      });
    }
    set(s => {
      const tabs = s.tabs.map(t => {
        const marked = !!t.collectionPath && stale.includes(t.collectionPath);
        return (t.staleOnDisk ?? false) === marked ? t : { ...t, staleOnDisk: marked };
      });
      if (tabs.every((t, i) => t === s.tabs[i])) return s;
      const active = tabs.find(t => t.id === s.activeTabId);
      return { tabs, staleOnDisk: active?.staleOnDisk ?? false };
    });
    return changed;
  },

  refreshCollections: async () => {
    try {
      const res = await fetch('/api/collections');
      if (!res.ok) { set({ collectionsRead: 'failed' }); return; }
      set({ collections: await res.json(), collectionsRead: 'ok' });
    } catch {
      set({ collectionsRead: 'failed' });
    }
    void get().refreshChanged();
  },

  setChangedSince: (ref) => {
    const since = ref.trim();
    writeText('play.changed.since', since);
    set({ changedSince: since || null });
    void get().refreshChanged();
  },

  checked: {},
  checkedSaid: null,
  recheck: async (path) => {
    try {
      const res = await fetch('/api/check', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ paths: [path] }),
      });
      if (!res.ok) return;
      const data = await res.json();
      set(s => ({ checked: mergeChecked(s.checked, [path], data.files ?? []) }));
    } catch { /* a mark that cannot be refreshed is dropped by the caller */ }
  },

  checkAll: async (paths) => {
    try {
      const res = await fetch('/api/check', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ paths }),
      });
      if (!res.ok) {
        set({ checkedSaid: await res.text().catch(() => `Server returned ${res.status}`) });
        return;
      }
      const data = await res.json();
      const files: CheckedFile[] = data.files ?? [];
      const asked = paths.length > 0 ? paths : Object.keys(get().checked);
      set(s => ({
        checked: paths.length > 0
          ? mergeChecked(s.checked, asked, files)
          : Object.fromEntries(files.map(file => [file.path, file])),
        checkedSaid: checkSummary(files, data.checked ?? files.length, !!data.truncated),
      }));
    } catch (err: any) {
      set({ checkedSaid: err?.message || String(err) });
    }
  },

  refreshChanged: async () => {
    try {
      const since = get().changedSince;
      const res = await fetch(since ? `/api/changed?since=${encodeURIComponent(since)}` : '/api/changed');
      const data = res.ok ? await res.json() : null;
      set({
        changedPaths: data?.available ? data.paths : null,
        changedSince: data?.since ?? get().changedSince,
        changedAvailable: !!data?.available,
      });
    } catch {
      set({ changedPaths: null, changedAvailable: false });
    }
  },
}));

const stored = loadHistoryFromStorage();
if (stored.length > 0) {
  useStore.getState().setHistory(stored);
}

watchSystemTheme(() => {
  const s = useStore.getState();
  if (s.mode === 'system') s.setMode('system');
});
