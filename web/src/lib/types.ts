import type { ProjectEnvFile } from './project-env';

import type { Mode, ModePref, PaletteId } from 'luvo/theme/themes';
import type { RunState } from './jobs';
export type WireProtocol = 'grpc' | 'grpc-web' | 'connectrpc';

export function defaultAddressFor(protocol: WireProtocol): string {
  switch (protocol) {
    case 'connectrpc':
    case 'grpc-web':
      return 'localhost:4769';
    default:
      return 'localhost:4770';
  }
}

export function dialledAddress(
  address: string,
  protocol: WireProtocol,
  fromServer?: string | null,
  fromEnvironment?: string | null,
): string {
  return address.trim()
    || (fromEnvironment ?? '').trim()
    || (fromServer ?? '').trim()
    || defaultAddressFor(protocol);
}

export interface ServerEnv {
  address?: string | null;
  tls_ca?: string | null;
  tls_cert?: string | null;
  tls_key?: string | null;
  tls_server_name?: string | null;
  compression?: string | null;
}

export interface RequestConfig {
  endpoint: string;
  headers: Record<string, string>;
  bodies: string[];
}

export interface BenchMetricRow {
  name: string;
  baseline: number;
  current: number;
  abs_delta: number;
  pct_delta: number;
  verdict: string;
}

export interface BenchComparison {
  overall: 'pass' | 'fail';
  metrics: BenchMetricRow[];
  per_endpoint: BenchMetricRow[];
}

export interface RunAssertionResult {
  line: number;
  expression: string;
  passed: boolean;
  elapsed_ms: number;
  message: string | null;
  expected: string | null;
  actual: string | null;
  endpoint?: string | null;
  hint?: string | null;
}

export interface CallResult {
  status: 'ok' | 'error' | 'pending';
  statusCode: number | null;

  messages: unknown[];
  messageOffsetsMs?: number[];
  headers: Record<string, string>;
  trailers: Record<string, string>;
  error: string | null;
  durationMs: number | null;
  assertions?: RunAssertionResult[];
  fromRun?: boolean;
  shape?: string | null;
  messagesRaw?: string[];
  fromCase?: string;
  messagesTotal?: number;
  messagesTruncated?: boolean;
  fromStep?: number;
  sent?: boolean;
}

export interface HistoryEntry {
  id: string;
  timestamp: number;
  endpoint: string;
  bodies: string[];
  headers: Record<string, string>;
  response: CallResult;
  connection?: Connection;
  resolved?: string[];
  kind?: 'run';
  checks?: { passed: number; total: number };
  collectionPath?: string;
  datasetRow?: number;
}

export interface Connection {
  address: string;
  protocol?: WireProtocol;
  tls: boolean;
  tlsInsecure?: boolean;
}

export interface GctfMeta {
  name?: string;
  summary?: string;
  owner?: string;
  tags?: string[];
  links?: string[];
}

export type RunScope = 'file' | 'folder' | 'all';

export interface CollectionItem {
  path: string;
  name: string;
  is_dir: boolean;
  tags?: string[];
  mtime_ms?: number;
}

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
  tags?: string[];
}

export interface MethodInfo {
  name: string;
  full_name: string;
  input_type: string;
  output_type: string;
  client_streaming: boolean;
  server_streaming: boolean;
}

export interface ServiceInfo {
  name: string;
  full_name: string;
  methods: MethodInfo[];
}

export interface ReflectionMethod {
  name: string;
  fullName: string;
  service: string;
  clientStreaming: boolean;
  serverStreaming: boolean;
}

export interface ReflectResponse {
  services?: ServiceInfo[];
  error?: string | null;
}

export interface ProtoSourceResponse {
  source?: string | null;
  error?: string | null;
}

export interface SectionAttribute {
  section: string;
  index: number;
  name: string;
  value: string;
}

export interface CollectionParsed {
  endpoint: string;
  parallel?: boolean;
  address: string;
  headers: Record<string, string>;
  bodies: string[];
  sections_as_written?: Record<string, string>;
  bodies_as_written?: string[];
  asserts: string[];
  extracts: Record<string, string>;
  extract_types?: Record<string, string>;
  meta_name: string | null;
  meta_tags: string[];
  meta_owner: string | null;
  meta_summary: string | null;
  meta_links: string[];
  tls: Record<string, string>;
  options: Record<string, string>;
  bench: Record<string, string>;
  proto: Record<string, string>;
  dataset: unknown[];
  attributes: SectionAttribute[];
  bodies_stream?: boolean;
  expect_responses: ExpectMessage[];
  expect_error: ExpectMessage | null;
}

export interface ExpectMessage {
  body: string;
  partial: boolean;
  unordered_arrays: boolean;
  with_asserts: boolean;
  tolerance: number | null;
  redact: string[];
}

export type RequestTab =
  | 'body' | 'headers' | 'asserts' | 'extracts'
  | 'options' | 'tls' | 'meta' | 'proto' | 'dataset' | 'bench' | 'source' | 'plan'
  | 'config';
export type GctfTab = 'request' | 'raw' | 'asserts' | 'extracts' | 'try' | 'meta' | 'proto';
export type ResponseTab = 'response' | 'headers' | 'assertions';

export interface Environment {
  name: string;
  address?: string;
  variables: Record<string, string>;
  source?: 'project' | 'browser';
  secret?: string[];

  mutedVariables?: string[];
  tls?: boolean;
  tlsCa?: string;
  tlsCert?: string;
  tlsKey?: string;
  tlsInsecure?: boolean;
}

export const ENVS_KEY = 'grpctestify-envs';
export const ACTIVE_ENV_KEY = 'grpctestify-active-env';
export const TABS_KEY = 'grpctestify-tabs';
export const SETTINGS_KEY = 'grpctestify-settings';
export const RECENT_ADDRESS_KEY = 'grpctestify-recent-addresses';

export interface ClientSettings {
  address: string;
  protocol: WireProtocol;
  tls: boolean;
  tlsInsecure: boolean;
  tlsCa: string;
  tlsCert: string;
  tlsKey: string;
  requestTimeoutMs: number;
}

export interface ShareState {
  id: string;
  endpoint: string;
  headers: Record<string, string>;
  bodies: string[];
  address: string | null;
  protocol: string | null;
  tls: boolean | null;
  tls_insecure: boolean | null;
  created_at: number;
  expires_at: number;
  access_count: number;
  redacted?: string[];
}

export interface VariableUse {
  name: string;
  files: string[];
  count: number;
}

export interface ProjectDefaults {
  address: string;
  protocol: WireProtocol;
  tls: boolean;
  tlsInsecure: boolean;
  activeEnv: string | null;
}

export interface EnvLocalStatus {
  exists: boolean;
  content: string | null;
  secret: string[];
}

export interface DocumentSummary {
  index: number;
  endpoint: string;
  parallel?: boolean;
  kind: 'unary' | 'server' | 'client' | 'bidi';
  address: string;
  address_source: 'section' | 'inherited';
  headers: Record<string, string>;
  bodies: string[];
  asserts: string[];
  extracts: Record<string, string>;
  extract_types?: Record<string, string>;
  options: Record<string, string>;
  tls: Record<string, string>;
  proto: Record<string, string>;
  produces: string[];
  consumes: string[];
  start_line?: number;
  end_line?: number;
}

export type RunMode = 'execute' | 'run';

export interface Tab {
  id: string;
  label: string;
  endpoint: string;
  otherEndpoint?: string;
  headers: Record<string, string>;
  bodies: string[];
  response: CallResult | null;
  requestTab: RequestTab;
  gctfTab: GctfTab;
  responseTab: ResponseTab;
  collectionPath: string | null;
  collectionParsed: CollectionParsed | null;
  collectionOriginal: CollectionParsed | null;
  documents?: DocumentSummary[];
  staleOnDisk?: boolean;
  parseError?: string | null;
  rawContent: string | null;
  rawOriginal: string | null;
  originHistoryId?: string;
  isPreview?: boolean;
  protocolTouched?: boolean;
  addressTouched?: boolean;
  address?: string;
  runMode?: RunMode;
}

export interface GctfDiagnosticPosition {
  line: number;
  character: number;
}

export interface GctfDiagnostic {
  range: { start: GctfDiagnosticPosition; end: GctfDiagnosticPosition };
  severity?: number;
  message: string;
  code?: string | number;
  source?: string;
  data?: { scope?: string; [key: string]: unknown };
}

export interface StoredTab {
  i: string;
  l: string;
  e: string;
  h: Record<string, string>;
  b: string[];
  c: string | null;
  r?: string;
  m?: 'run';
  d?: string;
}

export interface TabsStorage {
  t: StoredTab[];
  a: string | null;
  r?: string;
}

export interface PlayStore {
  address: string;
  protocol: WireProtocol;
  tls: boolean;
  tlsInsecure: boolean;
  tlsCa: string;
  tlsCert: string;
  tlsKey: string;
  collections: CollectionItem[];
  collectionsRead: 'pending' | 'ok' | 'failed';

  layout: 'columns' | 'rows';
  setLayout: (layout: 'columns' | 'rows') => void;
  visibleFiles: string[];
  setVisibleFiles: (paths: string[]) => void;
  runFilter: 'all' | 'pass' | 'fail' | 'skip';
  runReason: string | null;
  reportFormats: string[];
  toggleReportFormat: (format: string) => void;
  lastReports: { jobId: string; files: string[] };
  setRunFilter: (mode: PlayStore['runFilter']) => void;
  setRunReason: (reason: string | null) => void;
  runScope: RunScope;
  runData: string | null;
  setRunData: (path: string | null, columns?: string[]) => void;
  runDataColumns: string[];
  setRunScope: (scope: RunScope) => void;
  run: RunState;
  runJobId: string | null;
  startRun: (paths: string[], upToStep?: number) => Promise<void>;
  adoptRunningJob: () => Promise<void>;
  scaffoldTest: (endpoint?: string) => Promise<void>;
  saveRawAs: (path: string, fmt?: boolean) => Promise<void>;
  startBench: (paths: string | string[]) => Promise<void>;
  cancelRun: () => Promise<void>;

  tabs: Tab[];

  activeTabId: string | null;

  workspacePath: string | null;

  workspaceOriginal: CollectionParsed | null;

  selectedCollection: string | null;
  collectionParsed: CollectionParsed | null;
  documents: DocumentSummary[];
  activeStep: number;
  datasetRow: number;
  executeBound: Record<string, [string, string][]>;
  setDatasetRow: (index: number) => void;
  setActiveStep: (index: number) => void;
  headParsed: CollectionParsed | null;
  selectStep: (index: number) => boolean;
  discardEdits: () => Promise<boolean>;
  addAssert: (expr: string) => 'added' | 'duplicate' | 'empty';
  removeAssert: (index: number) => void;
  replaceAssert: (index: number, line: string) => void;
  setExpectMode: (mode: 'none' | 'response' | 'error') => void;
  setExpectResponse: (index: number, patch: Partial<ExpectMessage>) => void;
  addExpectResponse: () => void;
  expectFromResponse: () => boolean;
  focusAnswerStep: () => boolean;
  removeExpectResponse: (index: number) => void;
  setExpectError: (patch: Partial<ExpectMessage>) => void;
  addExtract: (name: string, expr: string) => void;
  removeExtract: (name: string) => void;
  setSectionKv: (section: 'options' | 'tls' | 'proto' | 'bench', kv: Record<string, string>) => void;
  setMetaField: (field: 'meta_name' | 'meta_summary' | 'meta_owner', value: string) => void;
  setMetaTags: (tags: string[]) => void;
  setMetaLinks: (links: string[]) => void;
  setDataset: (rows: unknown[]) => void;
  renameDatasetColumn: (from: string, to: string) => number;
  renameExtractVariable: (
    from: string,
    to: string,
    options?: { dataset?: boolean },
  ) => Promise<{ rewritten: number } | { refused: string }>;
  rawContent: string | null;
  rawOriginal: string | null;
  rawError: string | null;
  parseError: string | null;
  staleOnDisk: boolean;
  syncOpenFiles: () => Promise<string[]>;
  request: RequestConfig;
  requestTab: RequestTab;
  gctfTab: GctfTab;
  response: CallResult | null;
  responseTab: ResponseTab;
  history: HistoryEntry[];
  totalOk: number;
  totalError: number;
  version: string;
  sessionId: string;
  palette: PaletteId;
  mode: ModePref;
  themeMode: Mode;
  reflectionMethods: ReflectionMethod[];
  reflectStatus: 'idle' | 'loading' | 'ok' | 'error';
  reflectedAt: number | null;
  reflectError: string | null;
  reflectedAddress: string | null;
  serverHealthy: boolean;
  buildMoved: boolean;
  collectionsMtime: number;
  environments: Environment[];
  browserEnvs: Environment[];
  projectEnvs: Environment[];
  activeEnvironment: string | null;
  sidebarVisible: boolean;
  showHotkeyHelp: boolean;
  runStatus: 'idle' | 'running';
  runMode: RunMode;

  requestTimeoutMs: number;
  addressTouched: boolean;
  protocolTouched: boolean;
  setAddress: (v: string) => void;
  nameAddressInFile: () => boolean;
  setProtocol: (v: WireProtocol) => void;
  setTls: (v: boolean) => void;
  setTlsInsecure: (v: boolean) => void;
  setTlsCa: (v: string) => void;
  setTlsCert: (v: string) => void;
  setTlsKey: (v: string) => void;
  setRequestTimeoutMs: (v: number) => void;
  setEndpoint: (v: string) => void;
  setCallKind: (kind: 'grpc' | 'http') => void;
  setRequestBody: (idx: number, v: string) => void;
  addRequestBody: () => void;
  removeRequestBody: (idx: number) => void;
  moveRequestBody: (from: number, to: number) => void;
  duplicateRequestBody: (idx: number) => void;
  setRequestBodies: (v: string[]) => void;
  setRequestHeaders: (v: Record<string, string>) => void;
  setRequestTab: (v: RequestTab) => void;
  setGctfTab: (v: GctfTab) => void;
  problemCount: number;
  setProblemCount: (n: number) => void;
  diagnostics: GctfDiagnostic[];
  diagnosedText: string | null;
  setDiagnostics: (list: GctfDiagnostic[], text: string) => void;
  sidebarTab: 'collections' | 'history';
  showSidebarTab: (tab: 'collections' | 'history') => void;
  drawerOpen: boolean;
  setDrawerOpen: (open: boolean) => void;
  jqSeed: { expr: string; n: number } | null;
  openJq: (expr: string) => void;
  envManager: { defineVar: string | null; value?: string } | null;
  openEnvManager: (defineVar?: string | null, value?: string) => void;
  closeEnvManager: () => void;
  responseMessage: number;
  setResponseMessage: (index: number) => void;
  revealLine: number | null;
  revealInRaw: (line: number) => void;
  clearReveal: () => void;
  setResponseTab: (v: ResponseTab) => void;
  setCollections: (v: CollectionItem[]) => void;
  setCollectionParsed: (v: CollectionParsed | null) => void;
  setPalette: (v: PaletteId) => void;
  setMode: (v: ModePref) => void;
  getGrpcurlCommand: () => Promise<string>;
  getCurlCommand: () => string;
  loadCollection: (path: string, options?: { pin?: boolean }) => Promise<boolean>;
  duplicateCollection: (path: string) => Promise<string | null>;
  formatFile: () => Promise<number>;
  hydrateStaleTabs: () => Promise<void>;
  focusHeldCall: (call: { endpoint: string; headers: Record<string, string>; bodies: string[] }) => boolean;
  newWorkspace: () => void;
  saveConflict: { path: string; mine: string; theirs: string; raw: boolean } | null;
  resolveSaveConflict: (choice: 'overwrite' | 'reload' | 'cancel') => Promise<void>;
  saveWorkspace: () => Promise<boolean>;
  saveIntent: number;
  requestSave: () => void;
  saveAsIntent: number;
  requestSaveAs: () => void;
  discardIntent: number;
  requestDiscard: () => void;
  pickIntent: number;
  requestPick: () => void;
  recentAddresses: string[];
  changedPaths: string[] | null;
  checked: Record<string, import('./checked').CheckedFile>;
  checkedSaid: string | null;
  runRefused: { text: string; nonce: number } | null;
  checkAll: (paths: string[]) => Promise<void>;
  recheck: (path: string) => Promise<void>;
  changedSince: string | null;
  changedAvailable: boolean;
  setChangedSince: (ref: string) => void;
  refreshChanged: () => Promise<void>;
  serverEnv: ServerEnv;
  lastCallAddress: string | null;
  runError: string | null;
  benchBaseline: unknown | null;
  benchBaselinePath: string | null;
  benchPaths: string[];
  benchBaselinePartial: boolean;
  benchOverUnsaved: string[];
  benchComparison: BenchComparison | null;
  workspaceName: string;
  sharesPath: string;
  importIntent: number;
  importPrefill: string | null;
  share: {
    headers: Record<string, boolean>;
    ttl: number;
    link?: string;
    expires?: string;
  } | null;
  startShare: () => 'link' | 'dialog' | 'none';
  closeShare: () => void;
  shareCreated: (link: string, expires: string) => void;
  toggleShareHeader: (key: string) => void;
  setShareTtl: (ttl: number) => void;
  docsOpen: boolean;
  setDocsOpen: (v: boolean) => void;
  closeIntent: number;
  closeAllIntent: number;
  tabListIntent: number;
  rememberAddress: (v: string) => void;
  retargetPath: (from: string, to: string) => void;
  editChain: (op: 'append' | 'delete', index?: number) => Promise<string | null>;
  compareBench: () => Promise<void>;
  requestImport: (command?: string) => void;
  requestCloseTab: () => void;
  requestCloseAllTabs: () => void;
  requestTabList: () => void;
  openDroppedFile: (name: string, content: string) => void;
  saveWorkspaceAs: (name: string, meta?: GctfMeta, fmt?: boolean) => Promise<void>;
  previewSave: (path: string, meta?: GctfMeta, fmt?: boolean) =>
    Promise<{ content: string; current: string | null; error?: undefined } | { error: string; content?: undefined; current?: undefined }>;
  execute: () => Promise<void>;
  runTest: () => Promise<void>;
  setRunMode: (v: RunMode) => void;
  loadRawContent: () => Promise<void>;
  setRawContent: (v: string) => void;
  saveRawContent: () => Promise<boolean>;
  loadStartupInfo: () => Promise<string | null>;
  startupNote: string | null;
  dismissStartupNote: () => void;
  setReflectionMethods: (v: ReflectionMethod[]) => void;
  reflect: () => Promise<void>;
  checkHealth: () => Promise<void>;
  setActiveEnvironment: (name: string | null) => void;
  addEnvironment: (env: Environment) => void;
  updateEnvironment: (name: string, env: Environment) => void;
  deleteEnvironment: (name: string) => void;
  cancel: () => void;
  restoreHistory: (entry: HistoryEntry, options?: { pin?: boolean }) => void;
  pinTab: (tabId: string) => void;
  setHistory: (v: HistoryEntry[]) => void;
  forgetHistory: (id: string) => void;
  clearHistory: () => void;
  toggleSidebar: () => void;
  setShowHotkeyHelp: (v: boolean) => void;
  refreshCollections: () => Promise<void>;

  addTab: (config?: Partial<Omit<Tab, 'id'>>) => string | null;
  moveTab: (from: number, to: number) => void;
  removeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  getTabLabel: (id: string) => string;
  setTabLabel: (id: string, label: string) => void;

  projectRoot: string | null;
  projectEnvNames: string[];
  projectRootAbs: string | null;
  collectionsDir: string | null;
  projectDefaults: ProjectDefaults | null;
  saveProjectSettings: (s: { address?: string; protocol?: string; tls?: boolean; tls_insecure?: boolean; active_env?: string | null }) => Promise<boolean>;
  refreshProjectEnvs: () => Promise<void>;
  fetchVariableUses: () => Promise<VariableUse[]>;
  fetchProjectEnv: (name: string) => Promise<ProjectEnvFile>;
  saveProjectEnv: (name: string, content: string) => Promise<void>;
  fetchProjectEnvLocal: (name: string) => Promise<EnvLocalStatus>;
  saveProjectEnvLocal: (name: string, content: string) => Promise<void>;
  deleteProjectEnv: (name: string) => Promise<void>;
  deleteProjectEnvLocal: (name: string) => Promise<void>;
}
