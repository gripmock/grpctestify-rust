import type { CollectionItem, TreeNode } from './types';

export function buildTree(items: CollectionItem[]): TreeNode[] {
  const root: TreeNode = { name: '', path: '', isDir: true, children: [] };
  for (const item of items) {
    const parts = item.path.split('/');
    let node = root;
    for (let i = 0; i < parts.length; i++) {
      const isLeaf = i === parts.length - 1;
      const child = node.children.find(c => c.name === parts[i]);
      if (child) {
        if (isLeaf && !item.is_dir && child.isDir) {
          child.isDir = false;
          child.tags = item.tags;
        }
        node = child;
        continue;
      }
      const newNode: TreeNode = {
        name: parts[i],
        path: parts.slice(0, i + 1).join('/'),
        isDir: isLeaf ? item.is_dir : true,
        children: [],
        tags: isLeaf && !item.is_dir ? item.tags : undefined,
      };
      node.children.push(newNode);
      node = newNode;
    }
  }
  return root.children;
}

export type FixtureRole = 'setup' | 'teardown';

const FAMILY = /\.(gctf|httf|apif)$/i;

export function fixtureRole(path: string): FixtureRole | null {
  const name = path.split('/').pop() ?? '';
  if (!FAMILY.test(name)) return null;
  const stem = name.replace(FAMILY, '');
  if (stem === '_setup') return 'setup';
  if (stem === '_teardown') return 'teardown';
  return null;
}

const RANK: Record<string, number> = { setup: 0, teardown: 2 };

export function sortTree(nodes: TreeNode[]): TreeNode[] {
  return [...nodes].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    const rank = RANK[fixtureRole(a.name) ?? ''] ?? 1;
    const other = RANK[fixtureRole(b.name) ?? ''] ?? 1;
    if (rank !== other) return rank - other;
    return a.name.localeCompare(b.name);
  }).map(n => ({ ...n, children: sortTree(n.children) }));
}

export interface TagFilter {
  include: Set<string>;
  exclude: Set<string>;
}

export const NO_TAGS: TagFilter = { include: new Set(), exclude: new Set() };

export function filterTree(nodes: TreeNode[], q: string, tags: TagFilter): TreeNode[] {
  return nodes.reduce<TreeNode[]>((acc, n) => {
    if (n.isDir) {
      const children = filterTree(n.children, q, tags);
      if (children.length > 0) acc.push({ ...n, children });
    } else {
      const needle = q.toLowerCase();
      const own = n.tags || [];
      const matchText = !q
        || n.path.toLowerCase().includes(needle)
        || n.name.toLowerCase().includes(needle)
        || own.some(t => t.toLowerCase().includes(needle));
      const wanted = [...tags.include].every(t => own.includes(t));
      const refused = own.some(t => tags.exclude.has(t));
      if (matchText && wanted && !refused) acc.push(n);
    }
    return acc;
  }, []);
}

export function collectTags(items: CollectionItem[]): { tag: string; count: number }[] {
  const map = new Map<string, number>();
  for (const item of items) {
    for (const t of item.tags || []) map.set(t, (map.get(t) || 0) + 1);
  }
  return [...map.entries()]
    .map(([tag, count]) => ({ tag, count }))
    .sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag));
}

export function fileCount(node: TreeNode): number {
  if (!node.isDir) return 1;
  return node.children.reduce((n, child) => n + fileCount(child), 0);
}

export function filterByVerdict(
  nodes: TreeNode[],
  verdicts: Record<string, { state: string; message?: string }>,
  mode: 'all' | 'pass' | 'fail' | 'skip',
  reason?: string | null,
): TreeNode[] {
  if (mode === 'all' && !reason) return nodes;
  return nodes.reduce<TreeNode[]>((acc, n) => {
    if (n.isDir) {
      const children = filterByVerdict(n.children, verdicts, mode, reason);
      if (children.length > 0) acc.push({ ...n, children });
      return acc;
    }
    const verdict = verdicts[n.path];
    const byState = mode === 'all' || verdict?.state === mode;
    const byReason = !reason || (verdict?.state === 'fail' && failureReason(verdict.message) === reason);
    if (byState && byReason) acc.push(n);
    return acc;
  }, []);
}

export function failureReason(message: string | undefined | null): string {
  const lines = (message ?? '').split('\n').map(l => l.trim()).filter(Boolean);
  const heading = /^[\w ]+(?:error|failed)\s*:(?:\s*[\w ]+(?:error|failed)\s*:)*$/i;
  const meat = lines.find(line => !heading.test(line)) ?? lines[0] ?? '';

  return meat
    .replace(/^-\s*/, '')
    .replace(/\s*\(assertion at line \d+\)/g, '')
    .replace(/\s+at line \d+/g, '')
    .replace(/\s*\(Values:.*$/, '')
    .replace(/^(.*?:\s)\1/, '$1')
    .trim();
}

export interface FailureGroup {
  reason: string;
  paths: string[];
}

export function failureGroups(
  verdicts: Record<string, { state: string; message?: string; path?: string }>,
): FailureGroup[] {
  const groups = new Map<string, string[]>();
  for (const [path, verdict] of Object.entries(verdicts)) {
    if (verdict.state !== 'fail') continue;
    const reason = failureReason(verdict.message);
    if (reason === '') continue;
    const paths = groups.get(reason) ?? [];
    paths.push(verdict.path ?? path);
    groups.set(reason, paths);
  }
  return [...groups.entries()]
    .map(([reason, paths]) => ({ reason, paths }))
    .sort((a, b) => b.paths.length - a.paths.length || a.reason.localeCompare(b.reason));
}

const SAID = 3;
const saidCache = new WeakMap<object, Set<string>>();

export function reasonsSaidOnce(
  verdicts: Record<string, { state: string; message?: string; path?: string }>,
): Set<string> {
  const known = saidCache.get(verdicts);
  if (known) return known;
  const said = new Set(
    failureGroups(verdicts)
      .filter(g => g.paths.length > 1)
      .slice(0, SAID)
      .map(g => g.reason),
  );
  saidCache.set(verdicts, said);
  return said;
}

export function railSaysWhy(
  message: string | undefined,
  said: Set<string>,
  offersFix: boolean,
): boolean {
  return offersFix || !said.has(failureReason(message));
}

export function saveFolders(dirs: TreeNode[], query: string): TreeNode[] {
  const q = query.trim().toLowerCase();
  if (q !== '') {
    return dirs.filter(d => d.path.toLowerCase().includes(q) || d.name.toLowerCase().includes(q));
  }
  const kept = dirs.filter(d => fileCount(d) > 0);
  const parents = new Set<string>();
  for (const dir of kept) {
    const parts = dir.path.split('/');
    for (let i = 1; i < parts.length; i++) parents.add(parts.slice(0, i).join('/'));
  }
  return dirs.filter(d => fileCount(d) > 0 || parents.has(d.path));
}

export interface ActiveFilter {
  key: string;
  label: string;
  kind: 'include' | 'exclude' | 'changed' | 'problems';
}

export function activeFilters(
  filter: TagFilter,
  onlyChanged: boolean,
  since?: string | null,
  onlyProblems = false,
): ActiveFilter[] {
  const out: ActiveFilter[] = [];
  for (const tag of [...filter.include].sort()) out.push({ key: `+${tag}`, label: `+${tag}`, kind: 'include' });
  for (const tag of [...filter.exclude].sort()) out.push({ key: `-${tag}`, label: `−${tag}`, kind: 'exclude' });
  if (onlyChanged) {
    out.push({ key: 'changed', label: since ? `changed since ${since}` : 'only changed', kind: 'changed' });
  }
  if (onlyProblems) out.push({ key: 'problems', label: 'with problems', kind: 'problems' });
  return out;
}

export function childFolders(dirs: TreeNode[], parent: string): TreeNode[] {
  const prefix = parent === '' ? '' : `${parent}/`;
  return dirs.filter(d => {
    if (!d.path.startsWith(prefix)) return false;
    const rest = d.path.slice(prefix.length);
    return rest !== '' && !rest.includes('/');
  });
}

export interface Crumb {
  name: string;
  path: string;
}

export function crumbs(path: string, rootName = 'project root'): Crumb[] {
  const out: Crumb[] = [{ name: rootName, path: '' }];
  if (path === '') return out;
  const parts = path.split('/');
  for (let i = 0; i < parts.length; i++) {
    out.push({ name: parts[i], path: parts.slice(0, i + 1).join('/') });
  }
  return out;
}

export type FileFamily = 'gctf' | 'httf' | 'apif' | 'unknown';

export function familyOf(name: string): FileFamily {
  if (name.endsWith('.gctf')) return 'gctf';
  if (name.endsWith('.httf')) return 'httf';
  if (name.endsWith('.apif')) return 'apif';
  return 'unknown';
}

export function benchTakes(name: string): boolean {
  return familyOf(name) === 'gctf';
}

export function benchRefusal(name: string): string | null {
  switch (familyOf(name)) {
    case 'gctf': return null;
    case 'httf': return 'The load runner measures gRPC calls — an .httf has none';
    case 'apif': return 'The load runner measures gRPC calls — an .apif holds steps of both transports, so a bench takes a .gctf';
    default: return 'The load runner measures gRPC calls — this is not a .gctf';
  }
}

export function withFamilyExt(
  name: string,
  fallback: 'gctf' | 'httf' | 'apif' = 'gctf',
): string {
  const trimmed = name.trim();
  return familyOf(trimmed) === 'unknown' ? `${trimmed}.${fallback}` : trimmed;
}

export function saveExtFor(path: string | null | undefined): 'gctf' | 'httf' | 'apif' {
  const family = familyOf(path ?? '');
  return family === 'unknown' ? 'gctf' : family;
}

export function ancestorsOf(path: string): string[] {
  const parts = path.split('/').slice(0, -1);
  return parts.map((_, i) => parts.slice(0, i + 1).join('/'));
}

export function runTargets(node: TreeNode, visible: string[], all: string[] = []): string[] {
  const inside = new Set(pathsUnder(node));
  const chosen = visible.filter(p => inside.has(p));
  if (!node.isDir) return chosen;

  const dirs = new Set(chosen.filter(p => !fixtureRole(p)).map(dirOf));
  const held = new Set(chosen);
  const fixtures = all.filter(p =>
    !held.has(p) && fixtureRole(p) !== null && dirs.has(dirOf(p)));
  return [...chosen, ...fixtures];
}

function dirOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut === -1 ? '' : path.slice(0, cut);
}

function pathsUnder(node: TreeNode): string[] {
  if (!node.isDir) return [node.path];
  return node.children.flatMap(pathsUnder);
}

export function renameNote(from: string, to: string): string | null {
  const was = familyOf(from);
  const now = familyOf(to);
  if (was === now) return null;
  if (now === 'unknown') {
    return `${to} is not a test file — no run, check or bench picks it up, and it leaves the collection.`;
  }
  if (was === 'unknown') {
    return `${to} becomes a ${now} test: a run will pick it up, and its sections are read as one.`;
  }
  const grammar: Record<Exclude<FileFamily, 'unknown'>, string> = {
    gctf: 'a gRPC test — its ENDPOINT is read as package.Service/Method',
    httf: 'an HTTP test — its ENDPOINT is read as `<METHOD> /path`, and PROTO, TLS and ERROR are refused',
    apif: 'a mixed test — each step chooses its own transport from its own ENDPOINT',
  };
  return `${to} is ${grammar[now]}. What the file says is read by that grammar from now on.`;
}
