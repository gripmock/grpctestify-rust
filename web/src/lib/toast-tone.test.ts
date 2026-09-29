import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const read = (rel: string) => readFileSync(join(import.meta.dirname, '..', rel), 'utf8');

describe('the messages that are not failures', () => {
  it('a source that is gone is a warning', () => {
    expect(read('components/collections/RunBar.tsx'))
      .toMatch(/toast\.warn\(`\$\{runData\} is not on disk any more/);
  });

  it('a save that went through with problems in it is a warning', () => {
    expect(read('components/request/RequestPanel.tsx')).toMatch(/toast\.warn\(`Saved — \$\{count\(problems, 'problem'\)\}/);
  });

  it('taking the disk version is a warning', () => {
    expect(read('components/ui/ConflictDialog.tsx')).toMatch(/toast\.warn\(`\$\{path\} reloaded from disk/);
  });

  it('a save that did not go through is still a refusal', () => {
    expect(read('components/request/RequestPanel.tsx')).toMatch(/toast\.error\(err\?\.message \|\| 'Save failed'\)/);
  });
});
