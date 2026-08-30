import { describe, expect, it } from 'vitest';
import { reconcileChoice, rememberedChoice } from './run-data';

describe('a remembered source', () => {
  it('carries its columns back', () => {
    expect(rememberedChoice({ path: 'paths.csv', columns: ['paths.file'] }))
      .toEqual({ path: 'paths.csv', columns: ['paths.file'] });
  });

  it('is nothing when the shape is not one', () => {
    expect(rememberedChoice(null)).toBeNull();
    expect(rememberedChoice({ columns: ['a'] })).toBeNull();
    expect(rememberedChoice({ path: '' })).toBeNull();
    expect(rememberedChoice({ path: 'a.csv', columns: 'no' })).toEqual({ path: 'a.csv', columns: [] });
  });
});

describe('a choice checked against disk', () => {
  it('is dropped when the file is gone', () => {
    expect(reconcileChoice('paths.csv', [{ path: 'other.csv' }])).toBeNull();
  });

  it('takes the columns the source answers for now', () => {
    expect(reconcileChoice('paths.csv', [{ path: 'paths.csv', columns: ['paths.host'] }]))
      .toEqual({ path: 'paths.csv', columns: ['paths.host'] });
  });

  it('is nothing when nothing was chosen', () => {
    expect(reconcileChoice(null, [{ path: 'paths.csv' }])).toBeNull();
  });
});
