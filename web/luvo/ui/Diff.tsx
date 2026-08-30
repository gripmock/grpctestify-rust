import type { DiffLine } from 'luvo/data/diff';

/* Two characters, not one and a margin: the gap has to survive a copy, and a
   `margin-right` does not come along with the text. */
const MARK: Record<DiffLine['kind'], string> = { same: '  ', add: '+ ', del: '- ' };

/** Two versions of a text, line by line.
 *
 *  The mark is a character in the line, not a colour on it: a diff read on a
 *  colour-blind screen, printed, or pasted into a ticket said which lines were
 *  which only by their red and green, and what came out of a copy was the two
 *  versions interleaved with nothing to tell them apart. */
export function Diff({ lines, className }: { lines: DiffLine[]; className?: string }) {
  return (
    <pre className={className ? `diff ${className}` : 'diff'}>
      {lines.map((l, i) => (
        <span key={i} className={l.kind === 'same' ? undefined : l.kind}>
          <span className="diff-mark">{MARK[l.kind]}</span>
          <span className="diff-text">{l.text || ' '}</span>
        </span>
      ))}
    </pre>
  );
}
