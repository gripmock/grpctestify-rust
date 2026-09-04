export function apiPath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}
