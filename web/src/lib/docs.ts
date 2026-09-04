export interface DocPage {
  name: string;
  markdown: string;
}

export async function fetchDocs(paths: string[], jobId?: string | null): Promise<DocPage[]> {
  const res = await fetch('/api/docs', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ paths, ...(jobId ? { job_id: jobId } : {}) }),
  });
  if (!res.ok) throw new Error(await res.text().catch(() => `Server returned ${res.status}`));
  return res.json();
}

export function pageForHref(pages: DocPage[], href: string): number {
  const wanted = href.split('#')[0].split('/').pop() ?? href;
  return pages.findIndex(page => page.name === wanted);
}

export function pageTitle(name: string, markdown?: string): string {
  if (name === 'index.md') return 'overview';
  const heading = markdown?.match(/^#\s+(.+)$/m)?.[1]?.trim();
  return heading && heading !== 'API Documentation' ? heading : name.replace(/\.md$/, '');
}

export interface CommandRequest {
  endpoint: string;
  body: unknown;
  address?: string;
  protocol?: string;
  tls?: boolean;
  tls_insecure?: boolean;
  headers?: Record<string, string>;
  collection_path?: string;
}

export function runsTheWholeFile(command: string): boolean {
  return command.startsWith('grpctestify call ') && !command.includes(' -e ');
}

export async function commandLine(kind: 'call' | 'grpcurl', req: CommandRequest): Promise<string> {
  const res = await fetch(kind === 'call' ? '/api/call-command' : '/api/grpcurl', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  if (!res.ok) throw new Error(await res.text().catch(() => `Server returned ${res.status}`));
  const data = await res.json();
  return data.command ?? '';
}

export function matchingPages(pages: DocPage[], query: string): DocPage[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') return pages;
  return pages.filter(page =>
    page.name === 'index.md'
    || pageTitle(page.name, page.markdown).toLowerCase().includes(needle)
    || page.markdown.toLowerCase().includes(needle));
}
