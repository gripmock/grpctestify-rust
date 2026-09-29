import { describe, expect, it } from 'vitest';
import { addressOrigin, originClass } from './plan-source';

describe('where a step says its target came from', () => {
  it('is the file when the step names its own ADDRESS', () => {
    expect(addressOrigin({ own: true, fromChain: false, source: 'file' })).toBe('file');
  });

  it('is the file when an earlier step named it', () => {
    expect(addressOrigin({ own: false, fromChain: true, source: 'server' })).toBe('file');
  });

  it('is the environment when a project environment names it', () => {
    expect(addressOrigin({ own: false, fromChain: false, source: 'environment' })).toBe('environment');
  });

  it('is the workbench when nothing in the project names it', () => {
    expect(addressOrigin({ own: false, fromChain: false, source: 'server' })).toBe('workbench');
    expect(addressOrigin({ own: false, fromChain: false, source: 'default' })).toBe('workbench');
    expect(addressOrigin({ own: false, fromChain: false, source: 'typed' })).toBe('workbench');
  });

  it('names a class per origin', () => {
    expect(originClass('file')).toBe('plan-from is-file');
    expect(originClass('workbench')).toBe('plan-from is-workbench');
  });
});
