import type { TreeNode } from './types';
import { count } from 'luvo/data/plural';

export interface DeleteScope {
  files: number;
  unsaved: string[];
}

export function deleteScope(
  node: TreeNode,
  open: { path: string | null; dirty: boolean }[],
): DeleteScope {
  const files = countFiles(node);
  const prefix = node.isDir ? `${node.path}/` : node.path;
  const unsaved = open
    .filter(tab => tab.dirty && tab.path !== null
      && (node.isDir ? tab.path.startsWith(prefix) : tab.path === node.path))
    .map(tab => tab.path as string);
  return { files, unsaved: [...new Set(unsaved)] };
}

function countFiles(node: TreeNode): number {
  if (!node.isDir) return 1;
  return node.children.reduce((n, child) => n + countFiles(child), 0);
}

export function deleteQuestion(node: TreeNode, scope: DeleteScope): string {
  if (!node.isDir) return `Delete "${node.path}"? The file is removed from disk.`;
  const what = count(scope.files, 'file');
  return scope.files === 0
    ? `Delete "${node.path}"? The folder is removed from disk.`
    : `Delete "${node.path}"? ${what} inside go with it, removed from disk.`;
}

export function unsavedNote(scope: DeleteScope): string | null {
  if (scope.unsaved.length === 0) return null;
  return scope.unsaved.length === 1
    ? `${scope.unsaved[0]} has unsaved edits open — they go too.`
    : `${scope.unsaved.length} of them have unsaved edits open — they go too.`;
}

export function referencedNote(paths: string[]): string | null {
  if (paths.length === 0) return null;
  const where = paths.length <= 3
    ? paths.join(', ')
    : `${paths.slice(0, 3).join(', ')} and ${paths.length - 3} more`;
  const names = paths.length === 1 ? 'names' : 'name';
  return `${where} ${names} it — those files lose their schema.`;
}

export function renameBreaksNote(paths: string[]): string | null {
  if (paths.length === 0) return null;
  const where = paths.length <= 3
    ? paths.join(', ')
    : `${paths.slice(0, 3).join(', ')} and ${paths.length - 3} more`;
  const names = paths.length === 1 ? 'names' : 'name';
  return `${where} ${names} it by the old path and will not follow — those files lose their schema.`;
}
