import { looksHttp, splitEndpoint } from './http-endpoint';

export function tabTitle(
  tab: { label: string; endpoint: string },
  live?: string,
): string {
  if (tab.label !== 'Untitled') return tab.label;
  const endpoint = (live ?? tab.endpoint).trim();
  if (endpoint === '') return tab.label;
  if (looksHttp(endpoint)) {
    const { method, path } = splitEndpoint(endpoint);
    return `${method} ${path}`;
  }
  const method = endpoint.split('/').pop() ?? endpoint;
  return method.trim() === '' ? endpoint : method.trim();
}

export function titleIsBorrowed(tab: { label: string; endpoint: string }, live?: string): boolean {
  return tab.label === 'Untitled' && (live ?? tab.endpoint).trim() !== '';
}
