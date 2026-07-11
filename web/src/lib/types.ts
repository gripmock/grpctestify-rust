export type WireProtocol = 'grpc' | 'grpc-web' | 'connect';

export interface RequestConfig {
  endpoint: string;
  headers: Record<string, string>;
  bodies: string[];
}

export interface CallResult {
  status: 'ok' | 'error' | 'pending';
  statusCode: number | null;
  
  messages: unknown[];
  headers: Record<string, string>;
  trailers: Record<string, string>;
  error: string | null;
  durationMs: number | null;
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
export type GctfTab = 'request' | 'asserts' | 'extracts' | 'meta' | 'source' | 'proto';
export type ResponseTab = 'response' | 'headers';

export interface Environment {
  name: string;
  address?: string;
  variables: Record<string, string>;
  
  mutedVariables?: string[];
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
  response: CallResult | null;
  requestTab: RequestTab;
  gctfTab: GctfTab;
  responseTab: ResponseTab;
  collectionPath: string | null;
  collectionParsed: CollectionParsed | null;
  collectionOriginal: CollectionParsed | null;
}


export interface StoredTab {
  i: string;             
  l: string;             
  e: string;             
  h: Record<string, string>; 
  b: string[];           
  c: string | null;      
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
  environment: Record<string, string>;
  collections: CollectionItem[];

  
  tabs: Tab[];
  
  activeTabId: string | null;

  
  workspacePath: string | null;
  
  workspaceOriginal: CollectionParsed | null;
  
  selectedCollection: string | null;
  collectionParsed: CollectionParsed | null;
  request: RequestConfig;
  requestTab: RequestTab;
  gctfTab: GctfTab;
  response: CallResult | null;
  responseTab: ResponseTab;
  history: HistoryEntry[];
  version: string;
  sessionId: string;
  theme: 'light' | 'dark';
  reflectionMethods: { name: string; fullName: string; service: string }[];
  reflectStatus: 'idle' | 'loading' | 'ok' | 'error';
  serverHealthy: boolean;
  environments: Environment[];
  activeEnvironment: string | null;

  setAddress: (v: string) => void;
  setProtocol: (v: WireProtocol) => void;
  setTls: (v: boolean) => void;
  setTlsInsecure: (v: boolean) => void;
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
  newWorkspace: () => void;
  saveWorkspace: () => Promise<void>;
  saveWorkspaceAs: () => Promise<void>;
  isDirty: () => boolean;
  execute: () => Promise<void>;
  loadStartupInfo: () => Promise<void>;
  setReflectionMethods: (v: { name: string; fullName: string; service: string }[]) => void;
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
