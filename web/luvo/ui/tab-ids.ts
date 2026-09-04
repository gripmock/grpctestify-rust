export function tabIds(base: string, key: string): { tab: string; panel: string } {
  return { tab: `${base}-tab-${key}`, panel: `${base}-panel-${key}` };
}

export function tabPanelProps(base: string, key: string): { id: string; role: 'tabpanel'; 'aria-labelledby': string } {
  const ids = tabIds(base, key);
  return { id: ids.panel, role: 'tabpanel', 'aria-labelledby': ids.tab };
}
