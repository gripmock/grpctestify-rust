import { describe, it, expect } from 'vitest';
import { duplicateNames, filterNames, hiddenValue, overriddenRow, valueNamesVariable, missingNames, putAddress, rankMissing, rowState, rowsOf, shouldKeepLocal, splitRows, takeAddress, type Row } from './env-rows';

describe('what each file gets', () => {
  it('writes a plain variable to the shared file only', () => {
    expect(splitRows([{ key: 'HOST', value: 'h', local: false }]))
      .toEqual({ shared: [['HOST', 'h']], local: [] });
  });

  it('keeps the committed default while a local value overrides it', () => {
    expect(splitRows([{ key: 'TOKEN', value: 'mine', local: true, shared: 'placeholder' }]))
      .toEqual({ shared: [['TOKEN', 'placeholder']], local: [['TOKEN', 'mine']] });
  });

  it('keeps a machine-only value local and declares its name to the team', () => {
    expect(splitRows([{ key: 'TOKEN', value: 'mine', local: true }]))
      .toEqual({ shared: [['TOKEN', '']], local: [['TOKEN', 'mine']] });
  });

  it('ignores a row that was never named', () => {
    expect(splitRows([{ key: '  ', value: 'orphan', local: false }]))
      .toEqual({ shared: [], local: [] });
  });

  it('trims the name it writes', () => {
    expect(splitRows([{ key: ' HOST ', value: 'h', local: false }]).shared).toEqual([['HOST', 'h']]);
  });
});

describe('what the editor shows', () => {
  it('shows one row per variable, whichever file it came from', () => {
    expect(rowsOf([['A', '1']], [['B', '2']])).toEqual([
      { key: 'A', value: '1', local: false },
      { key: 'B', value: '2', local: true },
    ]);
  });

  it('shows the local value when both files name a variable', () => {
    expect(rowsOf([['TOKEN', 'placeholder']], [['TOKEN', 'mine']])).toEqual([
      { key: 'TOKEN', value: 'mine', local: true, shared: 'placeholder' },
    ]);
  });

  it('survives a round trip through the editor', () => {
    const rows = rowsOf([['A', '1'], ['TOKEN', 'placeholder']], [['TOKEN', 'mine']]);
    expect(splitRows(rows)).toEqual({
      shared: [['A', '1'], ['TOKEN', 'placeholder']],
      local: [['TOKEN', 'mine']],
    });
  });
});

describe('the address the environment dials', () => {
  it('is lifted out of the rows, and comes back as one', () => {
    const rows = rowsOf([['GRPC_ADDRESS', 'staging:4770'], ['A', '1']], []);
    const taken = takeAddress(rows);
    expect(taken.address).toBe('staging:4770');
    expect(taken.rows.map(r => r.key)).toEqual(['A']);
    expect(putAddress(taken.rows, taken.address, taken.addressLocal)).toEqual(rows);
  });

  it('keeps the address machine-only when that is where it was', () => {
    const rows = rowsOf([['GRPC_ADDRESS', 'shared:4770']], [['GRPC_ADDRESS', 'mine:4770']]);
    const taken = takeAddress(rows);
    expect(taken).toMatchObject({ address: 'mine:4770', addressLocal: true });
    expect(splitRows(putAddress(taken.rows, taken.address, taken.addressLocal)).local)
      .toEqual([['GRPC_ADDRESS', 'mine:4770']]);
  });

  it('takes a cleared address out of the file', () => {
    expect(putAddress([{ key: 'A', value: '1', local: false }], '  ', false).map(r => r.key)).toEqual(['A']);
  });
});

describe('what this file needs and the environment lacks', () => {
  it('names only what is missing', () => {
    const rows = rowsOf([['USER', 'ada']], []);
    expect(missingNames(['USER', 'TOKEN'], rows)).toEqual(['TOKEN']);
  });

  it('never asks for the address as a variable', () => {
    expect(missingNames(['GRPC_ADDRESS'], [])).toEqual([]);
  });
});

describe('a suite with more names than the panel holds', () => {
  const uses = [
    { name: 'RARE', count: 1 },
    { name: 'EVERYWHERE', count: 42 },
    { name: 'SOME', count: 7 },
  ];

  it('puts the names the most files ask for first', () => {
    expect(rankMissing(['RARE', 'SOME', 'EVERYWHERE'], uses)).toEqual(['EVERYWHERE', 'SOME', 'RARE']);
  });

  it('orders the ones nobody counts alphabetically', () => {
    expect(rankMissing(['b', 'a'], [])).toEqual(['a', 'b']);
  });

  it('filters from anywhere in the name', () => {
    expect(filterNames(['GRPC_TOKEN', 'USER'], 'tok')).toEqual(['GRPC_TOKEN']);
    expect(filterNames(['A'], '  ')).toEqual(['A']);
  });
});

describe('a value kept out of git', () => {
  it('leaves the name in the shared file and the value only in the local one', () => {
    const rows = [{ key: 'TOKEN', value: 'abc', local: true, shared: '' }];
    expect(splitRows(rows)).toEqual({ shared: [['TOKEN', '']], local: [['TOKEN', 'abc']] });
  });

  it('keeps a value the team file had of its own', () => {
    const rows = [{ key: 'HOST', value: 'localhost', local: true, shared: 'prod.example.com' }];
    expect(splitRows(rows)).toEqual({
      shared: [['HOST', 'prod.example.com']],
      local: [['HOST', 'localhost']],
    });
  });
});

describe('a target kept out of git', () => {
  it('leaves the name in the shared file', () => {
    const rows = putAddress([], 'localhost:9000', true);
    expect(splitRows(rows)).toEqual({
      shared: [['GRPC_ADDRESS', '']],
      local: [['GRPC_ADDRESS', 'localhost:9000']],
    });
  });

  it('keeps the team’s own target when there is one', () => {
    const rows = putAddress([], 'localhost:9000', true, 'prod.example.com:443');
    expect(splitRows(rows)).toEqual({
      shared: [['GRPC_ADDRESS', 'prod.example.com:443']],
      local: [['GRPC_ADDRESS', 'localhost:9000']],
    });
  });

  it('writes one line when the target is shared', () => {
    expect(splitRows(putAddress([], 'localhost:9000', false)))
      .toEqual({ shared: [['GRPC_ADDRESS', 'localhost:9000']], local: [] });
  });

  it('carries what the shared file said back out of the rows', () => {
    const lifted = takeAddress([{ key: 'GRPC_ADDRESS', value: 'mine', local: true, shared: 'theirs' }]);
    expect(lifted.addressShared).toBe('theirs');
    expect(takeAddress([{ key: 'GRPC_ADDRESS', value: 'mine', local: false }]).addressShared).toBeUndefined();
  });
});

describe('a value typed where the shared file left a blank', () => {
  it('is kept out of the committed file', () => {
    const rows = rowsOf([['TOKEN', ''], ['USER', 'Ada']], []);
    expect(rows[0].placeholderInShared).toBe(true);
    expect(rows[1].placeholderInShared).toBeUndefined();
    expect(shouldKeepLocal(rows[0], 'project', 'sec-ret')).toBe(true);
  });

  it('leaves a shared value that already had one alone', () => {
    const rows = rowsOf([['USER', 'Ada']], []);
    expect(shouldKeepLocal(rows[0], 'project', 'Grace')).toBe(false);
  });

  it('does not apply to a browser environment, which is nobody elses file', () => {
    const rows = rowsOf([['TOKEN', '']], []);
    expect(shouldKeepLocal(rows[0], 'browser', 'sec-ret')).toBe(false);
  });

  it('is not triggered by clearing a value', () => {
    const rows = rowsOf([['TOKEN', '']], []);
    expect(shouldKeepLocal(rows[0], 'project', '   ')).toBe(false);
  });
});

describe('the values the editor keeps covered', () => {
  it('are the local ones and the credential-shaped ones', () => {
    expect(hiddenValue({ key: 'TOKEN', value: 'x', local: false })).toBe(true);
    expect(hiddenValue({ key: 'HOST', value: 'x', local: true })).toBe(true);
    expect(hiddenValue({ key: 'HOST', value: 'x', local: false })).toBe(false);
  });
});

describe('rowState', () => {
  const row = (over: Partial<Row> = {}): Row => ({ key: 'TOKEN', value: '', local: false, ...over });

  it('says nothing about a row with a value', () => {
    expect(rowState(row({ value: 'abc' }))).toBe('set');
  });

  it('marks a name with no value at all', () => {
    expect(rowState(row())).toBe('empty');
  });

  it('marks the shared placeholder this machine has not filled', () => {
    expect(rowState(row({ placeholderInShared: true }))).toBe('awaiting-local');
  });

  it('says nothing about the row nobody has typed into', () => {
    expect(rowState(row({ key: '' }))).toBe('blank');
  });
});

describe('a name this environment defines twice', () => {
  const row = (key: string, value: string): Row => ({ key, value, local: false });

  it('is named once, however many times it is written', () => {
    expect(duplicateNames([row('A', '1'), row('B', '2'), row('A', '3')])).toEqual(['A']);
    expect(duplicateNames([row('A', '1'), row('A', '2'), row('A', '3')])).toEqual(['A']);
  });

  it('does not count the blank rows a form always has', () => {
    expect(duplicateNames([row('', ''), row('', ''), row('A', '1')])).toEqual([]);
  });

  it('marks the ones a later row replaces, and not the one that wins', () => {
    const rows = [row('A', '1'), row('B', '2'), row('A', '3')];
    expect(overriddenRow(rows, 0)).toBe(true);
    expect(overriddenRow(rows, 1)).toBe(false);
    expect(overriddenRow(rows, 2)).toBe(false);
  });
});

describe('a value that names another variable', () => {
  it('names what it asks for, once each', () => {
    expect(valueNamesVariable('Hello {{USER}}')).toEqual(['USER']);
    expect(valueNamesVariable('{{A}}/{{B}}/{{A}}')).toEqual(['A', 'B']);
  });

  it('is nothing for a plain value, or for braces that name nothing', () => {
    expect(valueNamesVariable('Ada')).toEqual([]);
    expect(valueNamesVariable('{{ }}')).toEqual([]);
    expect(valueNamesVariable('{{a-b}}')).toEqual([]);
  });
});
