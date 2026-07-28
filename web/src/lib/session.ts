const KEY = 'grpctestify-session';


export function getSessionId(): string {
  let id: string | null = null;
  try { id = localStorage.getItem(KEY); } catch {  }
  if (!id) {
    id = Math.random().toString(36).slice(2, 8);
    try { localStorage.setItem(KEY, id); } catch {  }
  }
  return id;
}
