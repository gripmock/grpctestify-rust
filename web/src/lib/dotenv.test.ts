import { describe, it, expect } from 'vitest';
import { applyEntries, entriesOf, parseDotenv, serializeDotenv } from './dotenv';

const FILE = `# staging
GRPC_ADDRESS=staging:4770

# the token the gateway checks
TOKEN="a b c"
export LEGACY=1
`;

describe('parseDotenv', () => {
  it('reads keys, values and quotes', () => {
    expect(entriesOf(parseDotenv(FILE))).toEqual([
      ['GRPC_ADDRESS', 'staging:4770'],
      ['TOKEN', 'a b c'],
      ['LEGACY', '1'],
    ]);
  });

  it('takes an inline comment off an unquoted value, and leaves a quoted one alone', () => {
    expect(entriesOf(parseDotenv('A=1 # one\nB="2 # two"'))).toEqual([['A', '1'], ['B', '2 # two']]);
  });

  it('keeps the last definition, as a reader does', () => {
    expect(entriesOf(parseDotenv('A=1\nA=2'))).toEqual([['A', '2']]);
  });
});

describe('round trip', () => {
  it('gives back the file it was handed', () => {
    expect(serializeDotenv(parseDotenv(FILE))).toBe(FILE);
  });

  it('keeps the comments and the order when a value changes', () => {
    const next = applyEntries(parseDotenv(FILE), [
      ['GRPC_ADDRESS', 'prod:4770'],
      ['TOKEN', 'a b c'],
      ['LEGACY', '1'],
    ]);
    expect(serializeDotenv(next)).toBe(FILE.replace('staging:4770', 'prod:4770'));
  });

  it('appends what is new and removes what is gone', () => {
    const next = applyEntries(parseDotenv('# head\nA=1\nB=2\n'), [['A', '1'], ['C', '3']]);
    expect(serializeDotenv(next)).toBe('# head\nA=1\nC=3\n');
  });

  it('quotes only what would not survive unquoted', () => {
    const next = applyEntries([], [['A', 'plain'], ['B', 'two words'], ['C', 'has#hash'], ['D', '']]);
    expect(serializeDotenv(next)).toBe('A=plain\nB="two words"\nC="has#hash"\nD=\n');
  });

  it('holds a value with a quote in it', () => {
    const text = serializeDotenv(applyEntries([], [['A', 'say "hi"']]));
    expect(entriesOf(parseDotenv(text))).toEqual([['A', 'say "hi"']]);
  });

  it('writes nothing for a row that was never named', () => {
    expect(serializeDotenv(applyEntries([], [['', 'orphan']]))).toBe('');
  });
});
