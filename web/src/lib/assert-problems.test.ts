import { describe, expect, it } from 'vitest';
import { droppedLines, keyProblem, methodProblem, missingPaths, problemsFor, unboundLines } from './assert-problems';
import type { GctfDiagnostic } from './types';

const at = (message: string): GctfDiagnostic => ({
  range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
  message,
});

describe('the problems about one assertion', () => {
  const list = [
    at('Assertion ends on `==` with nothing to compare against: .name =='),
    at('Assertion reads nothing from the answer, so it passes whatever comes back: 200'),
    at('At least one verification section (RESPONSE, ERROR, or ASSERTS) is required'),
  ];

  it('finds the one that names this line', () => {
    expect(problemsFor('.name ==', list)).toEqual([list[0]]);
    expect(problemsFor('200', list)).toEqual([list[1]]);
  });

  it('is nothing for a line nothing is said about', () => {
    expect(problemsFor('.name == "Ada"', list)).toEqual([]);
    expect(problemsFor('   ', list)).toEqual([]);
  });

  it('reads the line as written', () => {
    expect(problemsFor('  .name ==  ', list)).toEqual([list[0]]);
  });
});

describe('the EXTRACT lines that bind nothing', () => {
  it('reads the line out of the message that quotes it', () => {
    expect(unboundLines([
      at('EXTRACT line binds nothing — write `name = filter`: who'),
      at('EXTRACT line binds nothing — write `name = filter`: = .name'),
      at('Assertion reads nothing from the answer, so it passes whatever comes back: 200'),
    ])).toEqual(['who', '= .name']);
  });

  it('is nothing when the file has no such line', () => {
    expect(unboundLines([at('At least one verification section is required')])).toEqual([]);
  });
});

describe('the key-value lines a section drops', () => {
  it('reads the line out of the message, for the section asked about', () => {
    const list = [
      at('REQUEST_HEADERS line is not a `key: value` pair, so it is dropped: authorization Bearer t'),
      at('OPTIONS line is not a `key: value` pair, so it is dropped: timeout 5'),
    ];
    expect(droppedLines(list, 'REQUEST_HEADERS')).toEqual(['authorization Bearer t']);
    expect(droppedLines(list, 'OPTIONS')).toEqual(['timeout 5']);
    expect(droppedLines(list, 'TLS')).toEqual([]);
  });
});

describe('the paths a file names and the disk does not have', () => {
  it('reads the name and where it was looked for', () => {
    expect(missingPaths([
      at('PROTO names gone.bin, and there is nothing at /p/gone.bin'),
      at('TLS names /abs/ca.pem, and there is nothing there'),
    ], 'PROTO')).toEqual([{ named: 'gone.bin', at: '/p/gone.bin' }]);
    expect(missingPaths([
      at('TLS names /abs/ca.pem, and there is nothing there'),
    ], 'TLS')).toEqual([{ named: '/abs/ca.pem', at: null }]);
  });

  it('is nothing for a file whose paths are all there', () => {
    expect(missingPaths([at('Empty assertion found')], 'PROTO')).toEqual([]);
  });
});

describe('what check says about one key', () => {
  it('finds the diagnostic that names it', () => {
    const said = at("Unknown OPTIONS key 'timeuot' — did you mean 'timeout'? Supported keys: timeout");
    expect(keyProblem('timeuot', [said])).toBe(said.message);
    expect(keyProblem('timeout', [said])).toBeNull();
  });
});

describe('what check says about the method', () => {
  const said = at("'PSOT' is not one of the usual HTTP methods — it is sent as written. Hint: did you mean 'POST'?");

  it('finds the sentence about this verb, however it is typed', () => {
    expect(methodProblem('PSOT', [said])).toBe(said.message);
    expect(methodProblem(' psot ', [said])).toBe(said.message);
    expect(methodProblem('POST', [said])).toBeNull();
  });
});
