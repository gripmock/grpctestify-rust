import { useEffect } from 'react';
import { useStore } from './store';
import type { ShareState } from './types';

export function useDeepLink() {
  const loadCollection = useStore(s => s.loadCollection);
  const addTab = useStore(s => s.addTab);

  useEffect(() => {
    const path = window.location.pathname;

    const cMatch = path.match(/^\/c\/(.+)/);
    if (cMatch) {
      loadCollection(cMatch[1]);
      window.history.replaceState({}, '', '/');
      return;
    }

    const sMatch = path.match(/^\/s\/(.+)/);
    if (sMatch) {
      const shareId = sMatch[1];
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
