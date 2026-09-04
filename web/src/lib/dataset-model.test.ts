import { describe, it, expect } from 'vitest';
import { columnsOf, cellIn, cellOut, setCell, addColumn, addRow, pruneRows, datasetUsage, renameColumn, removeColumn, renameDatasetRefs, countDatasetRefs } from './dataset-model';

describe('columnsOf', () => {
  it('unions the keys in first-seen order', () => {
    expect(columnsOf([{ id: 1, name: 'a' }, { name: 'b', role: 'x' }])).toEqual(['id', 'name', 'role']);
  });
  it('ignores rows that are not objects', () => {
    expect(columnsOf([42, null, { id: 1 }])).toEqual(['id']);
  });
});

describe('cell values', () => {
  it('keeps numeric-looking ids as strings — an id is not arithmetic', () => {
    expect(cellIn('1')).toBe('1');
    expect(cellIn('007')).toBe('007');
  });
  it('parses the literals that are unambiguous', () => {
    expect(cellIn('true')).toBe(true);
    expect(cellIn('null')).toBeNull();
    expect(cellIn('[1,2]')).toEqual([1, 2]);
  });
  it('leaves malformed JSON as text rather than losing it', () => {
    expect(cellIn('{oops')).toBe('{oops');
  });
  it('round-trips through the cell', () => {
    expect(cellOut(cellIn('Ada'))).toBe('Ada');
    expect(cellOut(cellIn('[1,2]'))).toBe('[1,2]');
    expect(cellOut(undefined)).toBe('');
  });
});

describe('editing the grid', () => {
  const rows = [{ id: '1', name: 'Ada' }, { id: '2', name: 'Grace' }];

  it('sets one cell without touching the others', () => {
    expect(setCell(rows, 1, 'name', 'Hopper')).toEqual([{ id: '1', name: 'Ada' }, { id: '2', name: 'Hopper' }]);
  });

  it('clearing a cell removes the key rather than writing an empty string', () => {
    expect(setCell(rows, 0, 'name', '')).toEqual([{ id: '1' }, { id: '2', name: 'Grace' }]);
  });

  it('adds a column to every row', () => {
    expect(addColumn(rows, 'role')).toEqual([
      { id: '1', name: 'Ada', role: '' },
      { id: '2', name: 'Grace', role: '' },
    ]);
  });

  it('adds a column to an empty dataset by creating the first row', () => {
    expect(addColumn([], 'id')).toEqual([{ id: '' }]);
  });

  it('adds a row shaped like the existing columns', () => {
    expect(addRow(rows)).toHaveLength(3);
    expect(addRow(rows)[2]).toEqual({ id: '', name: '' });
  });
});

describe('pruneRows', () => {
  it('drops the blank cells the grid seeds', () => {
    expect(pruneRows([{ id: '1', email: '' }])).toEqual([{ id: '1' }]);
  });

  it('drops a row that is entirely blank', () => {
    expect(pruneRows([{ id: '', email: '' }, { id: '2' }])).toEqual([{ id: '2' }]);
  });

  it('keeps falsey values that are real', () => {
    expect(pruneRows([{ n: 0, ok: false, missing: null }])).toEqual([{ n: 0, ok: false, missing: null }]);
  });
});

describe('datasetUsage', () => {
  it('splits the columns the file reads from the ones it ignores', () => {
    const use = datasetUsage(['id', 'name', 'note'], ['{ "id": "{{dataset.id}}" }', 'x-name: {{ dataset.name }}']);
    expect(use.used).toEqual(['id', 'name']);
    expect(use.unused).toEqual(['note']);
    expect(use.missing).toEqual([]);
  });

  it('names a placeholder with no column behind it', () => {
    const use = datasetUsage(['id'], ['{{dataset.id}} {{dataset.missing}}']);
    expect(use.missing).toEqual(['missing']);
  });

  it('ignores placeholders from other sources', () => {
    const use = datasetUsage(['id'], ['{{TOKEN}} {{env.HOST}}']);
    expect(use.used).toEqual([]);
    expect(use.missing).toEqual([]);
  });

  it('names a placeholder written where nothing substitutes', () => {
    const use = datasetUsage(['id'], ['{ "id": "{{dataset.id}}" }'], ['.name == "{{dataset.id}}"']);
    expect(use.inert).toEqual(['id']);
    expect(use.missing).toEqual([]);
  });

  it('does not call a column unused when only an assert names it', () => {
    const use = datasetUsage(['name'], [], ['.name == "{{dataset.name}}"']);
    expect(use.unused).toEqual([]);
    expect(use.inert).toEqual(['name']);
  });

  it('is empty for a file with nothing in it', () => {
    expect(datasetUsage([], [])).toEqual({ used: [], unused: [], missing: [], inert: [] });
  });
});

describe('renameColumn', () => {
  it('carries every row’s value to the new name', () => {
    const rows = [{ id: '1', kind: 'a' }, { id: '2', kind: 'b' }];
    expect(renameColumn(rows, 'kind', 'sort')).toEqual([{ id: '1', sort: 'a' }, { id: '2', sort: 'b' }]);
  });

  it('keeps the column where it was', () => {
    expect(columnsOf(renameColumn([{ id: '1', kind: 'a', z: '9' }], 'kind', 'sort'))).toEqual(['id', 'sort', 'z']);
  });

  it('refuses a blank name and a rename to itself', () => {
    const rows = [{ id: '1' }];
    expect(renameColumn(rows, 'id', '  ')).toBe(rows);
    expect(renameColumn(rows, 'id', 'id')).toBe(rows);
  });
});

describe('removeColumn', () => {
  it('takes the column out of every row', () => {
    expect(removeColumn([{ id: '1', kind: 'a' }, { id: '2' }], 'kind')).toEqual([{ id: '1' }, { id: '2' }]);
  });
});

describe('renaming a column', () => {
  it('takes the references with it', () => {
    expect(renameDatasetRefs('GET /v1/{{dataset.id}}', 'id', 'user')).toBe('GET /v1/{{dataset.user}}');
    expect(renameDatasetRefs('{{ dataset.id }} and {{dataset.id}}', 'id', 'user'))
      .toBe('{{dataset.user}} and {{dataset.user}}');
  });

  it('leaves other variables and other columns alone', () => {
    expect(renameDatasetRefs('{{USER}} {{dataset.other}}', 'id', 'user')).toBe('{{USER}} {{dataset.other}}');
  });

  it('does nothing without a new name', () => {
    expect(renameDatasetRefs('{{dataset.id}}', 'id', '  ')).toBe('{{dataset.id}}');
    expect(renameDatasetRefs('{{dataset.id}}', 'id', 'id')).toBe('{{dataset.id}}');
  });

  it('counts what a rename would touch', () => {
    expect(countDatasetRefs(['{{dataset.id}}', 'x {{dataset.id}} y', '{{dataset.other}}'], 'id')).toBe(2);
    expect(countDatasetRefs([], 'id')).toBe(0);
  });
});
