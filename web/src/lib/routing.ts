import { useEffect } from 'react';
import { useStore } from './store';
import type { ShareState, WireProtocol } from './types';
import { nextUrl, parseDeepLink, urlWhenLinkFails } from './deeplink';
import { useToast } from 'luvo/ui/useToast';

export { parseDeepLink, encodeCollectionLink } from './deeplink';
export type { DeepLink } from './deeplink';

let handledDeepLink: string | null = null;
let pendingCollection: string | null = null;

export function useDeepLink() {
  const loadCollection = useStore(s => s.loadCollection);
  const addTab = useStore(s => s.addTab);
  const toast = useToast();

  useEffect(() => {
    const path = window.location.pathname;
    if (path === handledDeepLink) return;

    const link = parseDeepLink(path);
    if (!link) return;

    if (link.kind === 'collection') {
      handledDeepLink = path;
      pendingCollection = link.value;
      void loadCollection(link.value).then(opened => {
        if (!opened) {
          pendingCollection = null;
          window.history.replaceState({}, '', urlWhenLinkFails(useStore.getState().workspacePath));
          toast.error(`${link.value} is not in this workbench — it may have been renamed or removed`);
        }
      });
      return;
    }

    {
      handledDeepLink = path;
      const shareId = link.value;
      fetch(`/api/share/${shareId}`)
        .then(async res => {
          if (res.status === 404) {
            toast.error('That share does not exist — the link may be wrong, or it was removed');
            return;
          }
          if (res.status === 410) {
            toast.error('That share has expired — shares are kept for the days their author chose');
            return;
          }
          if (!res.ok) {
            toast.error(`The share could not be read (${res.status})`);
            return;
          }
          const data: ShareState = await res.json();
          addTab({
            endpoint: data.endpoint,
            headers: data.headers,
            bodies: data.bodies,
            label: data.endpoint || 'Shared',
          });
          const st = useStore.getState();
          if (data.address) st.setAddress(data.address);
          if (data.protocol) st.setProtocol(data.protocol as WireProtocol);
          if (data.tls !== null) st.setTls(data.tls);
          if (data.tls_insecure !== null) st.setTlsInsecure(data.tls_insecure);
          const where = data.address ? ` — pointed at ${data.address}` : '';
          toast.success(`Shared request opened${where}`);
          const kept = data.redacted ?? [];
          if (kept.length > 0) {
            toast.warn(`${kept.join(', ')} did not travel with the share — add it in HEADERS`);
          }
        })
        .catch(() => toast.error('The share could not be read — the workbench server did not answer'));
      window.history.replaceState({}, '', '/');
      return;
    }
  }, [loadCollection, addTab, toast]);
}

export function useUrlSync() {
  const workspacePath = useStore(s => s.workspacePath);
  const loadCollection = useStore(s => s.loadCollection);

  useEffect(() => {
    const next = nextUrl(window.location.pathname, workspacePath, pendingCollection);
    if (pendingCollection !== null && pendingCollection === workspacePath) pendingCollection = null;
    if (next !== null) window.history.replaceState({}, '', next);
  }, [workspacePath]);

  useEffect(() => {
    const onPop = () => {
      const link = parseDeepLink(window.location.pathname);
      if (link?.kind === 'collection') loadCollection(link.value);
    };
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, [loadCollection]);
}
