import { describe, it, expect } from 'vitest';
import { ERROR, INFO, WARNING, aboutWholeFile, blocksSave, countBySeverity, problemCensus, diagnosticsVoice, matchLine, matchLineExact, problemKey, problemMarkers, severityLabel, sortProblems } from './problems';
import type { GctfDiagnostic } from './types';

function d(line: number, severity?: number, message = 'boom', code?: string): GctfDiagnostic {
  return {
    range: { start: { line, character: 0 }, end: { line, character: 5 } },
    severity,
    message,
    code,
  };
}

describe('countBySeverity', () => {
  it('counts each bucket', () => {
    expect(countBySeverity([d(1, ERROR), d(2, WARNING), d(3, WARNING), d(4, INFO)]))
      .toEqual({ errors: 1, warnings: 2, infos: 1 });
  });

  it('treats a missing severity as an error, as LSP does', () => {
    expect(countBySeverity([d(1)])).toEqual({ errors: 1, warnings: 0, infos: 0 });
  });

  it('is zero for an empty list', () => {
    expect(countBySeverity([])).toEqual({ errors: 0, warnings: 0, infos: 0 });
  });
});

describe('sortProblems', () => {
  it('puts errors before warnings before infos', () => {
    const out = sortProblems([d(9, INFO), d(5, WARNING), d(7, ERROR)]);
    expect(out.map(p => p.severity)).toEqual([ERROR, WARNING, INFO]);
  });

  it('orders same-severity problems by position', () => {
    const out = sortProblems([d(9, ERROR), d(2, ERROR), d(5, ERROR)]);
    expect(out.map(p => p.range.start.line)).toEqual([2, 5, 9]);
  });

  it('does not mutate its input', () => {
    const input = [d(9, ERROR), d(2, ERROR)];
    sortProblems(input);
    expect(input.map(p => p.range.start.line)).toEqual([9, 2]);
  });
});

describe('severityLabel', () => {
  it('maps every severity, defaulting to error', () => {
    expect(severityLabel(ERROR)).toBe('error');
    expect(severityLabel(WARNING)).toBe('warning');
    expect(severityLabel(INFO)).toBe('info');
    expect(severityLabel(4)).toBe('info');
    expect(severityLabel(undefined)).toBe('error');
  });
});

describe('problemKey', () => {
  it('is stable as the problem moves down the file', () => {
    expect(problemKey(d(3, ERROR, 'same', 'SEM_T001')))
      .toBe(problemKey(d(40, ERROR, 'same', 'SEM_T001')));
  });

  it('separates different problems on the same line', () => {
    expect(problemKey(d(3, ERROR, 'one'))).not.toBe(problemKey(d(3, ERROR, 'two')));
  });
});

describe('blocksSave', () => {
  it('blocks on errors only', () => {
    expect(blocksSave([d(1, ERROR)])).toBe(true);
    expect(blocksSave([d(1, WARNING), d(2, INFO)])).toBe(false);
    expect(blocksSave([])).toBe(false);
  });
});

describe('matchLine', () => {
  const diagnosed = ['--- ENDPOINT ---', 'a.A/One', '', '--- REQUEST ---', '{}'].join('\n');

  it('is the same line when the two copies agree', () => {
    expect(matchLine(diagnosed, 1, diagnosed)).toBe(1);
  });

  it('follows the line when the other copy has shifted', () => {
    const shifted = ['--- META ---', 'name: x', '', diagnosed].join('\n');
    expect(matchLine(diagnosed, 1, shifted)).toBe(4);
  });

  it('prefers the nearest match when a line repeats', () => {
    const repeated = ['{}', '{}', '{}', '{}', '{}', '{}'].join('\n');
    expect(matchLine(diagnosed, 4, repeated)).toBe(4);
  });

  it('falls back to the line number when the text is gone or blank', () => {
    expect(matchLine(diagnosed, 1, 'nothing like it\nat all')).toBe(1);
    expect(matchLine(diagnosed, 2, 'a\nb\nc\nd')).toBe(2);
  });

  it('never points past the end of the target', () => {
    expect(matchLine(diagnosed, 4, 'one line')).toBe(0);
  });
});

describe('matchLineExact', () => {
  const diagnosed = ['--- ENDPOINT ---', 'a.A/One', '', '--- REQUEST ---', '{}'].join('\n');

  it('admits when the line has no counterpart in the file', () => {
    expect(matchLineExact(diagnosed, 1, '--- ENDPOINT ---\nb.B/Two')).toBe(-1);
    expect(matchLineExact(diagnosed, 2, 'a\nb\nc')).toBe(-1, );
  });

  it('finds the line that is there', () => {
    expect(matchLineExact(diagnosed, 1, ['x', 'y', 'a.A/One'].join('\n'))).toBe(2);
  });
});

describe('blocksSave', () => {
  const at = (severity: number, message: string): GctfDiagnostic => ({
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    severity,
    message,
  });

  it('is true when the file would not check', () => {
    expect(blocksSave([at(1, 'At least one verification section is required')])).toBe(true);
  });

  it('is false for warnings and notes', () => {
    expect(blocksSave([at(2, 'unknown OPTIONS key'), at(3, 'a note')])).toBe(false);
  });

  it('is false for a clean file', () => {
    expect(blocksSave([])).toBe(false);
  });
});

describe('a line that only the file can place', () => {
  it('is the file’s line, not the preview’s', () => {
    const preview = '--- ENDPOINT ---\na.Svc/M\n--- OPTIONS ---\ntimeout: 0\n';
    const file = '--- ENDPOINT ---\na.Svc/M\n\n--- REQUEST ---\n{}\n\n--- OPTIONS ---\ntimeout: 0\n';
    expect(matchLineExact(preview, 3, file)).toBe(7);
  });

  it('says so when the line exists in neither', () => {
    expect(matchLineExact('a\nb\n', 1, 'x\ny\n')).toBe(-1);
  });
});

describe('a problem about the whole file', () => {
  const at = (line: number, data?: Record<string, unknown>) => ({
    range: { start: { line, character: 0 }, end: { line, character: 4 } },
    severity: 2,
    message: 'x',
    ...(data ? { data } : {}),
  });

  it('is the one the server marked, not the one at line one', () => {
    expect(aboutWholeFile(at(0, { scope: 'file' }))).toBe(true);
    expect(aboutWholeFile(at(0))).toBe(false);
    expect(aboutWholeFile(at(0, { unknown_key: 'a' }))).toBe(false);
  });
});

describe('the marks in the editor', () => {
  const FILE = '--- ADDRESS ---\nlocalhost:4770\n\n--- OPTIONS ---\nnonsense: 1\n';
  const warn = (line: number, from: number, to: number) => ({
    range: { start: { line, character: from }, end: { line, character: to } },
    severity: 2,
    message: "Unknown OPTIONS key 'nonsense'",
  });

  it('are the diagnosed places when the text is the diagnosed text', () => {
    expect(problemMarkers([warn(4, 0, 8)], FILE, FILE)).toEqual([{
      startLine: 5, startColumn: 1, endLine: 5, endColumn: 9, severity: 2, message: "Unknown OPTIONS key 'nonsense'",
    }]);
  });

  it('follow the line to where it sits in this text', () => {
    const diagnosed = '--- ADDRESS ---\nlocalhost:4770\n\n--- META ---\ntags: [a]\n\n--- OPTIONS ---\nnonsense: 1\n';
    const [mark] = problemMarkers([warn(7, 0, 8)], diagnosed, FILE);
    expect({ line: mark.startLine, from: mark.startColumn, to: mark.endColumn }).toEqual({ line: 5, from: 1, to: 12 });
  });

  it('leave a line the file does not have unmarked', () => {
    const diagnosed = FILE + '\n--- ASSERTS ---\n.a == 1\n';
    expect(problemMarkers([warn(6, 0, 7)], diagnosed, FILE)).toEqual([]);
  });

  it('never mark a problem about the whole file', () => {
    const whole = { ...warn(0, 0, 4), data: { scope: 'file' } };
    expect(problemMarkers([whole], FILE, FILE)).toEqual([]);
  });

  it('mark nothing when nothing has been diagnosed', () => {
    expect(problemMarkers([warn(4, 0, 8)], null, FILE)).toEqual([]);
  });
});

describe('which reading a file is owed', () => {
  it('is the editor while the tab has edits the file does not', () => {
    expect(diagnosticsVoice({ path: 'a.gctf', dirty: true })).toBe('editor');
  });

  it('is the editor for a request that is not a file yet', () => {
    expect(diagnosticsVoice({ path: null, dirty: false })).toBe('editor');
  });

  it('is check once what is on screen is what is on disk', () => {
    expect(diagnosticsVoice({ path: 'a.gctf', dirty: false })).toBe('check');
  });
});

describe('what the save dialog says it found', () => {
  it('counts the notes beside the errors', () => {
    expect(problemCensus({ errors: 2, warnings: 1, infos: 0 }))
      .toBe('2 errors · 1 note — this file would not pass check');
    expect(problemCensus({ errors: 2, warnings: 1, infos: 1 }))
      .toBe('2 errors · 2 notes — this file would not pass check');
  });

  it('says errors alone when there are only errors', () => {
    expect(problemCensus({ errors: 1, warnings: 0, infos: 0 }))
      .toBe('1 error — this file would not pass check');
  });

  it('says notes alone when nothing is an error', () => {
    expect(problemCensus({ errors: 0, warnings: 2, infos: 1 })).toBe('3 notes');
    expect(problemCensus({ errors: 0, warnings: 1, infos: 0 })).toBe('1 note');
  });
});
