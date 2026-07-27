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

/** True when `address` is still the untouched default for `protocol` — the
 * signal `setProtocol` uses to decide whether to carry a manual override
 * forward instead of overwriting it with the new protocol's default. */
export function isAddressAtDefault(address: string, protocol: WireProtocol): boolean {
  return address === defaultAddressFor(protocol);
}

export interface RequestConfig {
  endpoint: string;
  headers: Record<string, string>;
  bodies: string[];
}

export interface RunAssertionResult {
  line: number;
  expression: string;
  passed: boolean;
  elapsed_ms: number;
  message: string | null;
  expected: string | null;
  actual: string | null;
}

export interface CallResult {
  status: 'ok' | 'error' | 'pending';
  statusCode: number | null;

  messages: unknown[];
  headers: Record<string, string>;
  trailers: Record<string, string>;
  error: string | null;
  durationMs: number | null;
  /** Set only when this result came from `/api/run` (full .gctf ASSERTS/EXTRACT run), not a raw `/api/call`. */
  assertions?: RunAssertionResult[];
}

export interface HistoryEntry {
  id: string;
  timestamp: number;
  endpoint: string;
  bodies: string[];
  headers: Record<string, string>;
  response: CallResult;
}

export interface CollectionItem {
  path: string;
  name: string;
  is_dir: boolean;
  tags?: string[];
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

/** Response contract for `POST /api/reflect` (see `ReflectResponse` in src/serve/api.rs). */
export interface ReflectResponse {
  services?: ServiceInfo[];
  error?: string | null;
}

/** Response contract for `POST /api/proto-source` (see `ProtoSourceResponse` in src/serve/api.rs). */
export interface ProtoSourceResponse {
  source?: string | null;
  error?: string | null;
}


export interface CollectionParsed {
  endpoint: string;
  address: string;
  headers: Record<string, string>;
  bodies: string[];
  asserts: string[];
  extracts: Record<string, string>;
  meta_name: string | null;
  meta_tags: string[];
  meta_owner: string | null;
  meta_summary: string | null;
  tls: Record<string, string>;
  options: Record<string, string>;
  bench: Record<string, string>;
  proto: Record<string, string>;
}

export type RequestTab = 'body' | 'headers' | 'env';
export type GctfTab = 'request' | 'raw' | 'asserts' | 'extracts' | 'meta' | 'proto';
export type ResponseTab = 'response' | 'headers' | 'assertions';

export interface Environment {
  name: string;
  address?: string;
  variables: Record<string, string>;

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
}


export interface ProjectInfo {
  active: boolean;
  envs: string[];
  collections_dir: string;
}


export interface ProjectSettings {
  address: string;
  protocol: WireProtocol;
  tls: boolean;
  tls_insecure: boolean;
  active_env: string | null;
}


export interface EnvLocalStatus {
  exists: boolean;
  content: string | null;
}


export interface Tab {
  id: string;
  label: string;
  endpoint: string;
  headers: Record<string, string>;
  bodies: string[];
  environment: Record<string, string>;
  response: CallResult | null;
  requestTab: RequestTab;
  gctfTab: GctfTab;
  responseTab: ResponseTab;
  collectionPath: string | null;
  collectionParsed: CollectionParsed | null;
  collectionOriginal: CollectionParsed | null;
  /** Full raw `.gctf` text, loaded lazily when the "Raw" tab is opened. */
  rawContent: string | null;
  rawOriginal: string | null;
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
}


export interface StoredTab {
  i: string;
  l: string;
  e: string;
  h: Record<string, string>;
  b: string[];
  c: string | null;
  v?: Record<string, string>;
}

export interface TabsStorage {
  t: StoredTab[];
  a: string | null;
}

export interface PlayStore {
  address: string;
  protocol: WireProtocol;
  tls: boolean;
  tlsInsecure: boolean;
  tlsCa: string;
  tlsCert: string;
  tlsKey: string;
  environment: Record<string, string>;
  collections: CollectionItem[];

  
  tabs: Tab[];
  
  activeTabId: string | null;

  
  workspacePath: string | null;
  
  workspaceOriginal: CollectionParsed | null;
  
  selectedCollection: string | null;
  collectionParsed: CollectionParsed | null;
  rawContent: string | null;
  rawOriginal: string | null;
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
  theme: 'light' | 'dark';
  reflectionMethods: ReflectionMethod[];
  reflectStatus: 'idle' | 'loading' | 'ok' | 'error';
  reflectError: string | null;
  serverHealthy: boolean;
  collectionsMtime: number;
  environments: Environment[];
  activeEnvironment: string | null;
  sidebarVisible: boolean;
  showHotkeyHelp: boolean;
  runStatus: 'idle' | 'running';
  runMode: 'execute' | 'run';

  requestTimeoutMs: number;
  setAddress: (v: string) => void;
  setProtocol: (v: WireProtocol) => void;
  setTls: (v: boolean) => void;
  setTlsInsecure: (v: boolean) => void;
  setTlsCa: (v: string) => void;
  setTlsCert: (v: string) => void;
  setTlsKey: (v: string) => void;
  setRequestTimeoutMs: (v: number) => void;
  setEndpoint: (v: string) => void;
  setRequestBody: (idx: number, v: string) => void;
  addRequestBody: () => void;
  removeRequestBody: (idx: number) => void;
  setRequestBodies: (v: string[]) => void;
  setRequestHeaders: (v: Record<string, string>) => void;
  setRequestTab: (v: RequestTab) => void;
  setGctfTab: (v: GctfTab) => void;
  setResponseTab: (v: ResponseTab) => void;
  setCollections: (v: CollectionItem[]) => void;
  setCollectionParsed: (v: CollectionParsed | null) => void;
  setEnvironment: (v: Record<string, string>) => void;
  setTheme: (v: 'light' | 'dark') => void;
  getGrpcurlCommand: () => Promise<string>;
  loadCollection: (path: string) => Promise<void>;
  hydrateStaleTabs: () => Promise<void>;
  newWorkspace: () => void;
  saveWorkspace: () => Promise<void>;
  saveWorkspaceAs: (name: string) => Promise<void>;
  execute: () => Promise<void>;
  runTest: () => Promise<void>;
  setRunMode: (v: 'execute' | 'run') => void;
  loadRawContent: () => Promise<void>;
  setRawContent: (v: string) => void;
  saveRawContent: () => Promise<void>;
  fetchDiagnostics: (content: string) => Promise<GctfDiagnostic[]>;
  loadStartupInfo: () => Promise<void>;
  setReflectionMethods: (v: ReflectionMethod[]) => void;
  reflect: () => Promise<void>;
  checkHealth: () => Promise<void>;
  setActiveEnvironment: (name: string | null) => void;
  addEnvironment: (env: Environment) => void;
  updateEnvironment: (name: string, env: Environment) => void;
  deleteEnvironment: (name: string) => void;
  muteVariable: (envName: string, key: string) => void;
  unmuteVariable: (envName: string, key: string) => void;
  cancel: () => void;
  restoreHistory: (entry: HistoryEntry) => void;
  setHistory: (v: HistoryEntry[]) => void;
  clearHistory: () => void;
  toggleSidebar: () => void;
  setShowHotkeyHelp: (v: boolean) => void;
  refreshCollections: () => Promise<void>;

  
  addTab: (config?: Partial<Omit<Tab, 'id'>>) => string;
  removeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  getTabLabel: (id: string) => string;
  setTabLabel: (id: string, label: string) => void;

  
  projectRoot: string | null;
  projectEnvNames: string[];
  saveProjectSettings: (s: { address?: string; protocol?: string; tls?: boolean; tls_insecure?: boolean; active_env?: string | null }) => Promise<void>;
  fetchProjectEnv: (name: string) => Promise<string>;
  saveProjectEnv: (name: string, content: string) => Promise<void>;
  fetchProjectEnvLocal: (name: string) => Promise<EnvLocalStatus>;
  saveProjectEnvLocal: (name: string, content: string) => Promise<void>;
  deleteProjectEnvLocal: (name: string) => Promise<void>;
}
