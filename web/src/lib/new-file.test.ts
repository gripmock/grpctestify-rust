import { describe, it, expect } from 'vitest';
import { newFileContent, NEW_FILE_ENDPOINT, NEW_HTTP_ENDPOINT } from './new-file';

describe('newFileContent', () => {
  it('carries the three sections a gctf file needs to be a test', () => {
    const text = newFileContent();
    expect(text).toContain('--- ENDPOINT ---');
    expect(text).toContain(NEW_FILE_ENDPOINT);
    expect(text).toContain('--- REQUEST ---');
    expect(text).toContain('--- ASSERTS ---');
  });

  it('starts an http file as a method, a path and a status', () => {
    const text = newFileContent('httf');
    expect(text).toContain(NEW_HTTP_ENDPOINT);
    expect(text).toContain('@status() == 200');
    expect(text).not.toContain('--- REQUEST ---');
  });

  it('never leaves the endpoint empty', () => {
    expect(newFileContent()).not.toMatch(/--- ENDPOINT ---\n\n/);
    expect(newFileContent('httf')).not.toMatch(/--- ENDPOINT ---\n\n/);
  });
});
