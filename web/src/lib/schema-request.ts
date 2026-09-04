import type { PlayStore } from './types';
import { activeEnvAddress, effectiveTls } from './store';
import { dialledAddress } from './types';
import { timeoutSeconds } from './format';

export interface SchemaRequest {
  address: string;
  endpoint: string;
  tls?: boolean;
  tls_insecure?: boolean;
  tls_ca?: string;
  tls_cert?: string;
  tls_key?: string;
  collection_path?: string;
  protocol?: string;
  timeout_seconds?: number;
}

export function schemaRequest(st: PlayStore, endpoint = st.request.endpoint): SchemaRequest {
  const { tls, tlsInsecure, tlsCa, tlsCert, tlsKey } = effectiveTls(st);
  return {
    address: dialledAddress(st.address, st.protocol, st.serverEnv.address, activeEnvAddress(st)),
    endpoint,
    tls: tls || undefined,
    tls_insecure: tls && tlsInsecure,
    tls_ca: tls ? (tlsCa || undefined) : undefined,
    tls_cert: tls ? (tlsCert || undefined) : undefined,
    tls_key: tls ? (tlsKey || undefined) : undefined,
    collection_path: st.selectedCollection || undefined,
    protocol: st.protocol || undefined,
    timeout_seconds: timeoutSeconds(st.requestTimeoutMs) || undefined,
  };
}
