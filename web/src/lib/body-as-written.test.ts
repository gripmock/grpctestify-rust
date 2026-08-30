import { describe, it, expect } from 'vitest';
import { bodyAsWritten, sectionAsWritten } from './body-as-written';
import type { CollectionParsed } from './types';

const parsed = (bodies_as_written?: string[]) =>
  ({ bodies_as_written } as unknown as CollectionParsed);

describe('the text a message is written as', () => {
  it('is the file’s own when it differs from what the editor shows', () => {
    const file = '{\n  // who\n  message: "hi",\n}';
    expect(bodyAsWritten(parsed([file]), 0, '{\n  "message": "hi"\n}'))
      .toEqual({ text: file, kind: 'json5' });
  });

  it('is nothing once the editor holds that text', () => {
    const file = '{ message: "hi" }';
    expect(bodyAsWritten(parsed([file]), 0, ' { message: "hi" } ')).toBeNull();
  });

  it('is nothing for a file that says what it shows', () => {
    expect(bodyAsWritten(parsed([]), 0, '{}')).toBeNull();
    expect(bodyAsWritten(parsed(), 0, '{}')).toBeNull();
    expect(bodyAsWritten(null, 0, '{}')).toBeNull();
  });
});

describe('the text a section is written as', () => {
  it('is the file’s own where the forms show something else', () => {
    const p = { sections_as_written: { ASSERTS: '# why\n.a == 1' } } as unknown as CollectionParsed;
    expect(sectionAsWritten(p, 'ASSERTS')).toBe('# why\n.a == 1');
    expect(sectionAsWritten(p, 'EXTRACT')).toBeNull();
    expect(sectionAsWritten(null, 'ASSERTS')).toBeNull();
  });
});

describe('the same JSON, written differently', () => {
  it('is layout, not JSON5', () => {
    const file = '{"v": 1}';
    expect(bodyAsWritten(parsed([file]), 0, '{\n  "v": 1\n}')).toEqual({ text: file, kind: 'layout' });
  });

  it('is JSON5 when the text is not JSON at all', () => {
    const file = '{ v: 1, }';
    expect(bodyAsWritten(parsed([file]), 0, '{\n  "v": 1\n}')).toEqual({ text: file, kind: 'json5' });
  });
});
