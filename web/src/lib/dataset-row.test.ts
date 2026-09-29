import { describe, expect, it } from 'vitest';
import { clampRow, rowLabel, rowValues, rowsOf } from './dataset-row';

const dataset = [{ who: 'World' }, { who: 'nobody', times: 2 }];

describe('the row one call is made with', () => {
  it('reads a row as the substitution reads it', () => {
    expect(rowValues(dataset, 0)).toEqual({ who: 'World' });
    expect(rowValues(dataset, 1)).toEqual({ who: 'nobody', times: '2' });
  });

  it('answers nothing for a row that is not there', () => {
    expect(rowValues(dataset, 5)).toBeNull();
    expect(rowValues([], 0)).toBeNull();
    expect(rowValues(undefined, 0)).toBeNull();
  });

  it('names a row the way the run panel names one', () => {
    expect(rowLabel(dataset, 0)).toBe('row 1 of 2 · who=World');
    expect(rowLabel(dataset, 1)).toBe('row 2 of 2 · who=nobody times=2');
  });

  it('keeps a picked row inside the file it belongs to', () => {
    expect(clampRow(dataset, 5)).toBe(1);
    expect(clampRow(dataset, -1)).toBe(0);
    expect(clampRow([], 3)).toBe(0);
  });

  it('keeps only the rows that are rows', () => {
    expect(rowsOf([{ a: 1 }, 'nope', null, ['x']])).toEqual([{ a: 1 }]);
  });
});
