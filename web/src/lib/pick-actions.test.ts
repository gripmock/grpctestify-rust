import { describe, it, expect } from 'vitest';
import { acrossStream, containerActions, isContainer, metaActions, numberAssert, roundedNote, statusAction, streamNote } from './pick-actions';

describe('containerActions', () => {
  it('asserts a list by its size', () => {
    expect(containerActions('.items', [1, 2, 3])[0].line).toBe('@len(.items) == 3');
  });

  it('asserts an object by its presence', () => {
    expect(containerActions('.user', { id: 1 })).toEqual([{ label: 'Assert present', line: '@has_value(.user)' }]);
  });

  it('knows a leaf from a container', () => {
    expect(isContainer([])).toBe(true);
    expect(isContainer({})).toBe(true);
    expect(isContainer('x')).toBe(false);
    expect(isContainer(null)).toBe(false);
  });
});

describe('a value across a streamed response', () => {
  const stream = [
    { index: 0, message: 'tick', meta: { kind: 'beat' } },
    { index: 1, message: 'tick', meta: { kind: 'beat' } },
  ];

  it('is the same when every message agrees', () => {
    expect(acrossStream(stream, '.message', 'tick')).toBe('same');
    expect(acrossStream(stream, '.meta.kind', 'beat')).toBe('same');
  });

  it('varies when the messages disagree', () => {
    expect(acrossStream(stream, '.index', 0)).toBe('varies');
  });

  it('is missing when a message does not carry it', () => {
    expect(acrossStream([{ a: 1 }, { b: 2 }], '.a', 1)).toBe('missing');
  });

  it('says nothing about a single message — there is nothing to disagree', () => {
    expect(acrossStream([{ index: 0 }], '.index', 0)).toBe('same');
    expect(streamNote('same', 1)).toBeNull();
  });

  it('names the count in what it says', () => {
    expect(streamNote('varies', 4)).toContain('4 messages');
    expect(streamNote('missing', 4)).toContain('4 messages');
  });

  it('reads a quoted key', () => {
    expect(acrossStream([{ 'odd key': 1 }, { 'odd key': 1 }], '.["odd key"]', 1)).toBe('same');
  });

  it('reads through an index', () => {
    expect(acrossStream([{ xs: [{ v: 1 }] }, { xs: [{ v: 2 }] }], '.xs[0].v', 1)).toBe('varies');
  });
});

describe('what can be asserted about metadata', () => {
  it('writes the helper the grammar has for each side', () => {
    expect(metaActions('headers', 'x-request-id', 'abc')).toEqual([
      { label: 'Assert equals "abc"', line: '@header("x-request-id") == "abc"' },
      { label: 'Assert present', line: '@has_header("x-request-id")' },
    ]);
    expect(metaActions('trailers', 'grpc-status', '0')[0].line).toBe('@trailer("grpc-status") == "0"');
  });

  it('offers only presence for an empty value', () => {
    expect(metaActions('headers', 'x-flag', '')).toEqual([
      { label: 'Assert present', line: '@has_header("x-flag")' },
    ]);
  });

  it('quotes a key that needs it', () => {
    expect(metaActions('headers', 'x "odd"', 'v')[1].line).toBe('@has_header("x \\"odd\\"")');
  });
});

describe('the status of an HTTP answer', () => {
  it('is asserted by the code it came back with', () => {
    expect(statusAction(201)).toEqual({ label: 'Assert the status is 201', line: '@status() == 201' });
    expect(statusAction(404).line).toBe('@status() == 404');
  });
});

describe('roundedNote', () => {
  it('refuses an equality on a number the panel cannot hold', () => {
    const wide = JSON.parse('{"id": 9007199254740993}').id as number;
    expect(roundedNote(wide)).toContain('not the one that came back');
  });

  it('says nothing about numbers that survive', () => {
    expect(roundedNote(7)).toBe(null);
    expect(roundedNote(9007199254740991)).toBe(null);
    expect(roundedNote(1.5)).toBe(null);
    expect(roundedNote('9007199254740993')).toBe(null);
  });

  it('refuses a whole number past what a double counts by ones', () => {
    expect(roundedNote(1.2345678901234568e+20)).not.toBe(null);
  });
});

describe('numberAssert', () => {
  it('offers the cast for a number that came back as a string', () => {
    expect(numberAssert('.expires_in', '3600')).toEqual({
      line: '.expires_in:number == 3600',
      label: 'Assert equals 3600 as a number',
    });
  });

  it('reads a negative and a decimal', () => {
    expect(numberAssert('.delta', '-4')?.line).toBe('.delta:number == -4');
    expect(numberAssert('.rate', '1.5')?.line).toBe('.rate:number == 1.5');
  });

  it('says nothing about text, or about a number that is already one', () => {
    expect(numberAssert('.name', 'ada')).toBeNull();
    expect(numberAssert('.n', 7)).toBeNull();
    expect(numberAssert('.v', '')).toBeNull();
    expect(numberAssert('.v', '12abc')).toBeNull();
  });
});
