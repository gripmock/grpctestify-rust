export async function fetchCollections(): Promise<{ path: string; name: string }[]> {
  const res = await fetch('/api/collections');
  if (!res.ok) return [];
  return res.json();
}

export async function fetchCollection(path: string): Promise<string | null> {
  const res = await fetch(`/api/collections/${path}`);
  if (!res.ok) return null;
  const data = await res.json();
  return data.content;
}

export async function saveCollection(path: string, content: string): Promise<boolean> {
  const res = await fetch('/api/save', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path, content }),
  });
  return res.ok;
}

export interface CallResponse {
  success: boolean;
  messages: unknown[];
  error: string | null;
}

export async function executeCall(endpoint: string, body: unknown, headers?: Record<string, string>): Promise<CallResponse> {
  const res = await fetch('/api/call', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ endpoint, body, headers }),
  });
  return res.json();
}
