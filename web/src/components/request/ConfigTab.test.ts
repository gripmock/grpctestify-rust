import { describe, it, expect } from 'vitest';
import { sectionSeed } from './ConfigTab';

describe('sectionSeed', () => {
  it('writes nothing into the sections that decide how a call is made', () => {
    expect(sectionSeed('options')).toBe(null);
    expect(sectionSeed('tls')).toBe(null);
    expect(sectionSeed('proto')).toBe(null);
  });

  it('gives BENCH the mode it cannot be edited without', () => {
    expect(sectionSeed('bench')).toEqual({ mode: 'fixed' });
  });

  it('starts a dataset with a row to type into', () => {
    expect(sectionSeed('dataset')).toEqual({});
  });
});
