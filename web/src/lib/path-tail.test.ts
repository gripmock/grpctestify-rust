import { describe, expect, it } from 'vitest';
import { pathTail } from './path-tail';

describe('a path in a slot narrower than it is', () => {
  it('is left alone when it fits', () => {
    expect(pathTail('/a/b/c.env', 48)).toBe('/a/b/c.env');
  });

  it('keeps the end, whole segments at a time', () => {
    const said = pathTail('/private/tmp/claude-501/-Users-someone-very-long/scratch/proj/.grpctestify/.env.example', 40);
    expect(said.startsWith('…/')).toBe(true);
    expect(said.endsWith('.env.example')).toBe(true);
    expect(said.length).toBeLessThanOrEqual(40);
  });

  it('keeps at least the file name, however long it is', () => {
    expect(pathTail('/a/b/an-extremely-long-file-name.env.example', 10))
      .toBe('…/an-extremely-long-file-name.env.example');
  });

  it('adds as many parents as fit', () => {
    expect(pathTail('/one/two/three/four.env', 20)).toBe('…/two/three/four.env');
  });
});
