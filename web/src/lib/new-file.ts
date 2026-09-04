export const NEW_FILE_ENDPOINT = 'package.Service/Method';
export const NEW_HTTP_ENDPOINT = 'GET /v1/health';

export function newFileContent(family: 'gctf' | 'httf' = 'gctf'): string {
  if (family === 'httf') {
    return [
      '--- ENDPOINT ---',
      NEW_HTTP_ENDPOINT,
      '',
      '--- ASSERTS ---',
      '@status() == 200',
      '',
    ].join('\n');
  }
  return [
    '--- ENDPOINT ---',
    NEW_FILE_ENDPOINT,
    '',
    '--- REQUEST ---',
    '{}',
    '',
    '--- ASSERTS ---',
    '.ok == true',
    '',
  ].join('\n');
}
