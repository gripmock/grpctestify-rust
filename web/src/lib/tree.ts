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

export function sortTree(nodes: TreeNode[]): TreeNode[] {
  return [...nodes].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    return a.name.localeCompare(b.name);
  }).map(n => ({ ...n, children: sortTree(n.children) }));
}

export function filterTree(nodes: TreeNode[], q: string, activeTags: Set<string>): TreeNode[] {
  return nodes.reduce<TreeNode[]>((acc, n) => {
    if (n.isDir) {
      const children = filterTree(n.children, q, activeTags);
      if (children.length > 0) acc.push({ ...n, children });
    } else {
      const matchText = !q || n.name.toLowerCase().includes(q.toLowerCase());
      const matchTags = activeTags.size === 0 || (n.tags || []).some(t => activeTags.has(t));
      if (matchText && matchTags) acc.push(n);
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
