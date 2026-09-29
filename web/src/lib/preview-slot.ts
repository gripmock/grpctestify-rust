import { isPristineTab, isTabDirty } from './store';
import type { Tab } from './types';

export function previewSlot(tabs: Tab[]): number {
  return tabs.findIndex(t => t.isPreview && !isTabDirty(t));
}

export function tabHoldingCall(
  tabs: Tab[],
  call: { endpoint: string; headers: Record<string, string>; bodies: string[] },
): Tab | undefined {
  const same = (a: Record<string, string>, b: Record<string, string>) => {
    const keys = Object.keys(a);
    return keys.length === Object.keys(b).length && keys.every(k => a[k] === b[k]);
  };
  return tabs.find(
    t =>
      t.collectionPath === null
      && t.endpoint === call.endpoint
      && t.bodies.length === call.bodies.length
      && t.bodies.every((body, i) => body === call.bodies[i])
      && same(t.headers, call.headers),
  );
}

export function tabAtStake(tab: Tab, dirty: boolean): boolean {
  if (dirty) return true;
  if (tab.collectionPath !== null) return false;
  return !isPristineTab(tab);
}
