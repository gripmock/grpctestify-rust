import { describe, it, expect } from 'vitest';
import { fromFileRelative, relativeToFile } from './relative-path';

describe('a path as the file that names it must spell it', () => {
  it('reaches back out of a folder', () => {
    expect(relativeToFile('auth/login.gctf', 'demo.proto')).toBe('../demo.proto');
    expect(relativeToFile('auth/deep/login.gctf', 'demo.proto')).toBe('../../demo.proto');
  });

  it('says nothing extra for a file beside it', () => {
    expect(relativeToFile('auth/login.gctf', 'auth/demo.proto')).toBe('demo.proto');
    expect(relativeToFile('login.gctf', 'demo.proto')).toBe('demo.proto');
  });

  it('crosses to a sibling folder', () => {
    expect(relativeToFile('auth/login.gctf', 'schema/demo.proto')).toBe('../schema/demo.proto');
  });

  it('leaves an absolute path alone', () => {
    expect(relativeToFile('auth/login.gctf', '/opt/schema.proto')).toBe('/opt/schema.proto');
  });

  it('reads back what it wrote', () => {
    for (const [file, target] of [
      ['auth/login.gctf', 'demo.proto'],
      ['auth/deep/login.gctf', 'demo.proto'],
      ['auth/login.gctf', 'auth/demo.proto'],
      ['auth/login.gctf', 'schema/demo.proto'],
      ['login.gctf', 'demo.proto'],
    ] as const) {
      expect(fromFileRelative(file, relativeToFile(file, target))).toBe(target);
    }
  });

  it('leaves a path alone when no file is open', () => {
    expect(relativeToFile(null, 'demo.proto')).toBe('demo.proto');
    expect(fromFileRelative(null, '../demo.proto')).toBe('../demo.proto');
  });
});
