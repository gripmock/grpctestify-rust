import { describe, it, expect } from 'vitest';
import { parseInline, parseMarkdown } from './markdown';

describe('the inline forms docs writes', () => {
  it('reads code, bold and links as values', () => {
    expect(parseInline('**Endpoint:** `a.B/C`')).toEqual([
      { kind: 'strong', text: 'Endpoint:' },
      { kind: 'text', text: ' ' },
      { kind: 'code', text: 'a.B/C' },
    ]);
    expect(parseInline('see [auth](auth.md) for more')).toEqual([
      { kind: 'text', text: 'see ' },
      { kind: 'link', text: 'auth', href: 'auth.md' },
      { kind: 'text', text: ' for more' },
    ]);
  });

  it('never produces markup', () => {
    expect(parseInline('<script>alert(1)</script>')).toEqual([
      { kind: 'text', text: '<script>alert(1)</script>' },
    ]);
  });
});

describe('the blocks docs writes', () => {
  it('reads headings, code fences and rules', () => {
    const blocks = parseMarkdown('# Title\n\nsome text\n\n```json\n{\n  "a": 1\n}\n```\n\n---\n');
    expect(blocks.map(b => b.kind)).toEqual(['heading', 'para', 'code', 'rule']);
    expect(blocks[0]).toMatchObject({ level: 1 });
    expect(blocks[2]).toMatchObject({ lang: 'json', text: '{\n  "a": 1\n}' });
  });

  it('reads a table with its header', () => {
    const [table] = parseMarkdown('| Service | Tests |\n|---|---|\n| [a](a.md) | 3 |\n| b | 1 |\n');
    expect(table.kind).toBe('table');
    if (table.kind !== 'table') return;
    expect(table.head.map(c => c.map(i => i.text))).toEqual([['Service'], ['Tests']]);
    expect(table.rows).toHaveLength(2);
    expect(table.rows[0][0][0]).toEqual({ kind: 'link', text: 'a', href: 'a.md' });
  });

  it('leaves a lone pipe alone', () => {
    expect(parseMarkdown('| not a table\n').map(b => b.kind)).toEqual(['para']);
  });
});

describe('the lists a page writes', () => {
  it('is one block, one item per line', () => {
    const [block] = parseMarkdown('- `.a == 1`\n- `.b == 2`\n');
    expect(block).toEqual({
      kind: 'list',
      items: [[{ kind: 'code', text: '.a == 1' }], [{ kind: 'code', text: '.b == 2' }]],
    });
  });

  it('ends where the list ends', () => {
    const blocks = parseMarkdown('Asserts:\n\n- one\n- two\n\nAfter.\n');
    expect(blocks.map(b => b.kind)).toEqual(['para', 'list', 'para']);
  });

  it('reads a link inside an item, the way a linked step is written', () => {
    const [block] = parseMarkdown('- [pkg.Svc](pkg.Svc.md)\n');
    expect(block).toEqual({
      kind: 'list',
      items: [[{ kind: 'link', text: 'pkg.Svc', href: 'pkg.Svc.md' }]],
    });
  });

  it('is not a list when the dash is inside a sentence', () => {
    expect(parseMarkdown('a - b\n').map(b => b.kind)).toEqual(['para']);
  });
});

describe('a value that would otherwise break the page it is written into', () => {
  it('reads a code span fenced by more than one backtick', () => {
    expect(parseInline('``has `one` inside``')).toEqual([
      { kind: 'code', text: 'has `one` inside' },
    ]);
    expect(parseInline('``` ``two`` ```')).toEqual([{ kind: 'code', text: '``two``' }]);
  });

  it('still reads the ordinary span, and the words around it', () => {
    expect(parseInline('a `b` c')).toEqual([
      { kind: 'text', text: 'a ' },
      { kind: 'code', text: 'b' },
      { kind: 'text', text: ' c' },
    ]);
  });

  it('keeps an escaped pipe inside one cell', () => {
    const [table] = parseMarkdown('| `who` |\n|---|\n| a\\|b |\n');
    expect(table).toEqual({
      kind: 'table',
      head: [[{ kind: 'code', text: 'who' }]],
      rows: [[[{ kind: 'text', text: 'a|b' }]]],
    });
  });
});
