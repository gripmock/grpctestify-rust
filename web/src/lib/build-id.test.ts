import { describe, expect, it } from 'vitest';
import { buildMoved, isStaleChunkError, loadedBuild } from './build-id';

const docWith = (html: string) => {
  const doc = document.implementation.createHTMLDocument('t');
  doc.body.innerHTML = html;
  return doc;
};

describe('the build a tab is running', () => {
  it('is the entry chunk the document loaded', () => {
    const doc = docWith('<script type="module" crossorigin src="/assets/index-abc123.js"></script>');
    expect(loadedBuild(doc)).toBe('assets/index-abc123.js');
  });

  it('reads an absolute src as the path the server serves', () => {
    const doc = docWith('<script type="module" src="http://localhost:8871/assets/index-abc123.js"></script>');
    expect(loadedBuild(doc)).toBe('assets/index-abc123.js');
  });

  it('is unknown when nothing says', () => {
    expect(loadedBuild(docWith('<p>no script</p>'))).toBeNull();
  });
});

describe('whether the server has moved on', () => {
  it('says so when the two builds differ', () => {
    expect(buildMoved('assets/index-a.js', 'assets/index-b.js')).toBe(true);
  });

  it('says nothing while they agree', () => {
    expect(buildMoved('assets/index-a.js', 'assets/index-a.js')).toBe(false);
  });

  it('says nothing when either side is unknown', () => {
    expect(buildMoved(null, 'assets/index-b.js')).toBe(false);
    expect(buildMoved('assets/index-a.js', undefined)).toBe(false);
    expect(buildMoved('assets/index-a.js', null)).toBe(false);
  });
});

describe('a chunk that is no longer served', () => {
  it('is told apart from a fault in the workbench', () => {
    expect(isStaleChunkError('Failed to fetch dynamically imported module: http://x/assets/vs-A.js')).toBe(true);
    expect(isStaleChunkError('Importing a module script failed.')).toBe(true);
    expect(isStaleChunkError('error loading dynamically imported module')).toBe(true);
    expect(isStaleChunkError("Cannot read properties of undefined (reading 'map')")).toBe(false);
  });
});
