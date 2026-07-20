import { useEffect } from 'react';
import { useStore } from './store';
import type { ShareState } from './types';
import { parseDeepLink } from './deeplink';

export { parseDeepLink, encodeCollectionLink } from './deeplink';
export type { DeepLink } from './deeplink';

// Dedupe React StrictMode's double-invoke (and any remount) by path.
let handledDeepLink: string | null = null;

export function useDeepLink() {
  const loadCollection = useStore(s => s.loadCollection);
  const addTab = useStore(s => s.addTab);

  useEffect(() => {
    const path = window.location.pathname;
    if (path === handledDeepLink) return;

    const link = parseDeepLink(path);
    if (!link) return;

    if (link.kind === 'collection') {
      handledDeepLink = path;
      loadCollection(link.value);
      window.history.replaceState({}, '', '/');
      return;
    }

    {
      handledDeepLink = path;
      const shareId = link.value;
      fetch(`/api/share/${shareId}`)
        .then(async res => {
          if (res.status === 404) {
            console.warn('Share not found');
            return;
          }
          if (res.status === 410) {
            console.warn('Share has expired');
            return;
          }
          if (!res.ok) return;
          const data: ShareState = await res.json();
          addTab({
            endpoint: data.endpoint,
            headers: data.headers,
            bodies: data.bodies,
            label: data.endpoint || 'Shared',
          });
        })
        .catch(() => {});
      window.history.replaceState({}, '', '/');
      return;
    }
  }, [loadCollection, addTab]);
}
