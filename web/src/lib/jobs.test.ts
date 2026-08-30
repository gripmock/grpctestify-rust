import { describe, expect, it } from 'vitest';
import { applyEvent, benchFailure, benchLine, caseNote, caseTitle, runRefusal, coverageNote, emptyRun, failureHeadline, failureLine, moreRowsNote, rollUp, runProgressLine, scopeFiles, slowNote, stepMarks, unsavedAmong, verdictLabel, verdictResponse, verdictResult } from './jobs';
import type { RunState } from './jobs';

describe('applyEvent', () => {
  it('counts the same file once however many times its event arrives', () => {
    let s = applyEvent(emptyRun(), { event: 'suite_start', testCount: 2 });
    s = applyEvent(s, { event: 'test_pass', testId: 'a.gctf', duration: 1 });
    s = applyEvent(s, { event: 'test_fail', testId: 'b.gctf' });
    const once = { done: s.done, passed: s.passed, failed: s.failed };
    s = applyEvent(s, { event: 'test_pass', testId: 'a.gctf', duration: 1 });
    s = applyEvent(s, { event: 'test_fail', testId: 'b.gctf' });
    expect({ done: s.done, passed: s.passed, failed: s.failed }).toEqual(once);
    expect(once).toEqual({ done: 2, passed: 1, failed: 1 });
  });

  it('counts each file once and keeps its verdict', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'suite_start', testCount: 2 });
    s = applyEvent(s, { event: 'test_start', testId: 'a.gctf' });
    s = applyEvent(s, { event: 'test_pass', testId: 'a.gctf', duration: 12 });
    s = applyEvent(s, { event: 'test_fail', testId: 'b.gctf', duration: 7, message: 'boom' });
    expect(s.total).toBe(2);
    expect(s.done).toBe(2);
    expect(s.passed).toBe(1);
    expect(s.failed).toBe(1);
    expect(s.verdicts['a.gctf'].state).toBe('pass');
    expect(s.verdicts['b.gctf'].message).toBe('boom');
  });

  it('takes the suite summary as final', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'suite_start', testCount: 1 });
    s = applyEvent(s, { event: 'suite_end', summary: { total: 1, passed: 1, failed: 0, skipped: 0, duration: 40 } });
    expect(s.finished).toBe(true);
    expect(s.durationMs).toBe(40);
  });

  it('starts over when a second run begins', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'suite_start', testCount: 1 });
    s = applyEvent(s, { event: 'test_fail', testId: 'a.gctf' });
    s = applyEvent(s, { event: 'suite_start', testCount: 3 });
    expect(s.failed).toBe(0);
    expect(Object.keys(s.verdicts)).toHaveLength(0);
    expect(s.total).toBe(3);
  });
});

describe('scopeFiles', () => {
  const all = ['auth/login.gctf', 'auth/logout.gctf', 'feed/crud.gctf'];

  it('runs one file, its folder, or everything', () => {
    expect(scopeFiles(all, 'file', 'auth/login.gctf')).toEqual(['auth/login.gctf']);
    expect(scopeFiles(all, 'folder', 'auth/login.gctf')).toEqual(['auth/login.gctf', 'auth/logout.gctf']);
    expect(scopeFiles(all, 'all', 'auth/login.gctf')).toEqual(all);
  });

  it('has nothing to run in a file scope with no open file', () => {
    expect(scopeFiles(all, 'file', null)).toEqual([]);
    expect(scopeFiles(all, 'folder', null)).toEqual([]);
  });
});

describe('rollUp', () => {
  it('counts only the files a run has reached', () => {
    const verdicts = {
      'auth/login.gctf': { path: 'auth/login.gctf', state: 'pass' as const },
      'auth/logout.gctf': { path: 'auth/logout.gctf', state: 'fail' as const },
    };
    expect(rollUp(['auth/login.gctf', 'auth/logout.gctf', 'auth/new.gctf'], verdicts))
      .toEqual({ passed: 1, failed: 1, skipped: 0, running: 0, touched: 2 });
    expect(rollUp(['feed/crud.gctf'], verdicts).touched).toBe(0);
  });
});

describe('verdictLabel', () => {
  it('says how many checks failed rather than how long it took', () => {
    expect(verdictLabel({ path: 'a', state: 'fail', durationMs: 8, assertions: [
      { line: 1, expression: '.ok', passed: true },
      { line: 2, expression: '.n == 1', passed: false },
    ] })).toBe('checks 1/2');
  });

  it('falls back when the failure was not an assertion', () => {
    expect(verdictLabel({ path: 'a', state: 'fail', message: 'connection refused' })).toBe('failed');
  });

  it('reports the duration of a pass', () => {
    expect(verdictLabel({ path: 'a', state: 'pass', durationMs: 12 })).toBe('12 ms');
  });

  it('tells a file the run never reached from one it cut off', () => {
    expect(verdictLabel({ path: 'a', state: 'skip' })).toBe('not run');
    expect(verdictLabel({ path: 'a', state: 'skip', interrupted: true })).toBe('cancelled');
  });

  it('says the same about a row-driven file', () => {
    expect(verdictLabel({ path: 'a', state: 'skip', cases: { total: 4, failed: 0 } })).toBe('not run');
    expect(verdictLabel({ path: 'a', state: 'skip', interrupted: true, cases: { total: 4, failed: 0 } }))
      .toBe('cancelled');
  });
});

describe('what a cancel leaves on a file', () => {
  it('carries the interruption from the event to the verdict', () => {
    const state = applyEvent(emptyRun(), {
      event: 'test_skip',
      testId: 'a.gctf',
      interrupted: true,
      message: 'Cancelled — the call had already gone out',
    });
    expect(state.verdicts['a.gctf'].interrupted).toBe(true);
    expect(state.verdicts['a.gctf'].state).toBe('skip');
  });

  it('leaves a file the run never reached unmarked', () => {
    const state = applyEvent(emptyRun(), {
      event: 'test_skip',
      testId: 'b.gctf',
      message: 'Cancelled before it ran',
    });
    expect(state.verdicts['b.gctf'].interrupted).toBeUndefined();
  });
});

describe('failureHeadline', () => {
  it('picks the detail line over the heading', () => {
    expect(failureHeadline("Validation failed:\n  - Method 'List' not found\n  - No response stream"))
      .toBe("Method 'List' not found");
  });

  it('keeps a single-line message as it is', () => {
    expect(failureHeadline('connection refused')).toBe('connection refused');
  });
});

describe('stepMarks', () => {
  it('marks the step that stopped the chain and skips the rest', () => {
    const marks = stepMarks({ path: 'a', state: 'fail', documents: [12, 8] }, 4);
    expect(marks.map(m => m.state)).toEqual(['pass', 'fail', 'skip', 'skip']);
    expect(marks[0].durationMs).toBe(12);
  });

  it('passes every step of a passing chain', () => {
    expect(stepMarks({ path: 'a', state: 'pass', documents: [1, 2, 3] }, 3).map(m => m.state))
      .toEqual(['pass', 'pass', 'pass']);
  });

  it('blames the first step when the file failed before any step reported', () => {
    expect(stepMarks({ path: 'a', state: 'fail' }, 4).map(m => m.state))
      .toEqual(['fail', 'skip', 'skip', 'skip']);
    expect(stepMarks({ path: 'a', state: 'fail', documents: [] }, 1).map(m => m.state))
      .toEqual(['fail']);
  });

  it('marks nothing while the file is still running or was never run', () => {
    expect(stepMarks({ path: 'a', state: 'running' }, 2).map(m => m.state)).toEqual(['none', 'none']);
    expect(stepMarks(undefined, 2).map(m => m.state)).toEqual(['none', 'none']);
  });
});

describe('failureLine', () => {
  it('reads like the run output — expression, expected vs actual, line', () => {
    const line = failureLine({ path: 'a', state: 'fail', assertions: [
      { line: 24, expression: '.scope == "read"', passed: false, expected: 'read', actual: 'read write' },
    ] });
    expect(line).toEqual({ text: '.scope == "read"', detail: 'expected "read", got "read write"', line: 24 });
  });

  it('falls back to the transport error when nothing asserted', () => {
    expect(failureLine({ path: 'a', state: 'fail', message: 'Validation failed:\n  - connection refused' }))
      .toEqual({ text: 'connection refused', detail: null, line: null });
  });

  it('has nothing to say about a pass', () => {
    expect(failureLine({ path: 'a', state: 'pass' })).toBeNull();
  });
});

describe('bench events', () => {
  it('keeps the ticks and the report on the run state', () => {
    let s: RunState = { ...emptyRun(), kind: 'bench' };
    s = applyEvent(s, { event: 'suite_start', testCount: 1 });
    s = applyEvent(s, { event: 'bench_progress', elapsed_s: 5.1, requests: 60625, errors: 0, rps: 11792.4, targetRps: 0, errorPct: 0 });
    expect(s.kind).toBe('bench');
    expect(s.benchProgress?.requests).toBe(60625);

    s = applyEvent(s, { event: 'bench_report', report: { summary: { count: 60625 } } });
    expect(s.benchReport?.summary.count).toBe(60625);
    expect(s.benchProgress?.requests).toBe(60625);
  });

  it('a second suite_start clears the previous report', () => {
    let s: RunState = applyEvent({ ...emptyRun(), kind: 'bench' }, { event: 'bench_report', report: { summary: {} } });
    s = applyEvent(s, { event: 'suite_start', testCount: 1 });
    expect(s.benchReport).toBeNull();
  });
});

describe('runProgressLine', () => {
  it('counts files through a run and names the failures', () => {
    const run = { ...emptyRun(), total: 12, done: 3 };
    expect(runProgressLine(run)).toBe('running 3/12');
    expect(runProgressLine({ ...run, failed: 2 })).toBe('running 3/12 · 2 failed');
  });

  it('says when the stream dropped and it is coming back', () => {
    expect(runProgressLine({ ...emptyRun(), total: 12, done: 3, lost: 2 }))
      .toBe('stream lost · reconnecting (2)');
  });

  it('says what a bench has measured so far', () => {
    const bench = { ...emptyRun(), kind: 'bench' as const };
    expect(runProgressLine(bench)).toBe('benching');
    expect(runProgressLine({
      ...bench,
      benchProgress: { elapsed_s: 4.6, requests: 900, errors: 0, rps: 200, targetRps: 0, errorPct: 0 },
    })).toBe('benching · 5 s · 900 req');
  });
});

describe('verdictResponse', () => {
  const failed = {
    path: 'a.gctf', state: 'fail' as const, durationMs: 12, message: 'assertion failed',
    assertions: [{ line: 3, expression: '.ok == true', passed: false }],
    response: { messages: [{ ok: false }], headers: { 'content-type': 'application/grpc' }, trailers: {}, error: null },
  };

  it('turns what the run captured into a response the panel can show', () => {
    const shown = verdictResponse(failed)!;
    expect(shown.status).toBe('error');
    expect(shown.messages).toEqual([{ ok: false }]);
    expect(shown.error).toBe('assertion failed');
    expect(shown.assertions).toHaveLength(1);
    expect(shown.fromRun).toBe(true);
  });

  it('is nothing for anything that did not fail', () => {
    expect(verdictResponse(undefined)).toBeNull();
    expect(verdictResponse({ path: 'a', state: 'pass' })).toBeNull();
    expect(verdictResponse({ path: 'a', state: 'skip' })).toBeNull();
  });

  it('still answers for a failure that carried nothing back', () => {
    const shown = verdictResponse({
      path: 'a.gctf', state: 'fail', durationMs: 3,
      message: 'Failed to start gRPC stream: transport error',
    })!;
    expect(shown.status).toBe('error');
    expect(shown.error).toBe('Failed to start gRPC stream: transport error');
    expect(shown.messages).toEqual([]);
    expect(shown.fromRun).toBe(true);
  });
});

describe('the failure a rail row shows', () => {
  const verdict = (a: any) => ({ path: 'a.gctf', state: 'fail' as const, assertions: [a] });

  it('is the pair for a comparison that fits', () => {
    const line = failureLine(verdict({
      line: 18, expression: '.message == "x"', passed: false, elapsed_ms: 1, expected: 'x', actual: 'y',
    }));
    expect(line).toEqual({ text: '.message == "x"', detail: 'expected "x", got "y"', line: 18 });
  });

  it('is the message when the comparison is two whole messages', () => {
    const line = failureLine(verdict({
      line: 11, expression: '--- RESPONSE ---', passed: false, elapsed_ms: 0,
      message: "Value mismatch at '$.message'",
      expected: '{\n  "a": 1\n}', actual: '{\n  "a": 2\n}',
    }));
    expect(line).toEqual({ text: 'RESPONSE', detail: "Value mismatch at '$.message'", line: 11 });
  });

  it('never returns more than one line of it', () => {
    const long = failureLine(verdict({
      line: 1, expression: 'e', passed: false, elapsed_ms: 0, message: 'x'.repeat(200),
    }));
    expect(long!.detail!.length).toBeLessThanOrEqual(90);
    expect(long!.detail!.endsWith('\u2026')).toBe(true);
  });
});

describe('a file expanded over rows', () => {
  const rows = (state: RunState) => state.verdicts['d.gctf'];

  it('marks the file the rows belong to', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'test_pass', testId: 'd.gctf#[row=0 dataset.id=1]', duration: 3 });
    s = applyEvent(s, { event: 'test_pass', testId: 'd.gctf#[row=1 dataset.id=2]', duration: 5 });
    expect(rows(s)?.state).toBe('pass');
    expect(rows(s)?.durationMs).toBe(8);
    expect(verdictLabel(rows(s)!)).toBe('2 rows');
  });

  it('fails the file when any row fails, and keeps that row\'s evidence', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'test_pass', testId: 'd.gctf#[row=0 dataset.id=1]', duration: 3 });
    s = applyEvent(s, { event: 'test_fail', testId: 'd.gctf#[row=1 dataset.id=2]', message: 'no such user' });
    expect(rows(s)?.state).toBe('fail');
    expect(rows(s)?.message).toBe('no such user');
    expect(rows(s)?.caseLabel).toBe('row 2 of 2 · dataset.id=2');
    expect(verdictLabel(rows(s)!)).toBe('1/2 rows');
  });

  it('is still running while any row is', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'test_fail', testId: 'd.gctf#[row=0 dataset.id=1]' });
    s = applyEvent(s, { event: 'test_start', testId: 'd.gctf#[row=1 dataset.id=2]' });
    expect(rows(s)?.state).toBe('running');
  });

  it('counts every row, not every file', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'test_pass', testId: 'd.gctf#[row=0 dataset.id=1]', duration: 1 });
    s = applyEvent(s, { event: 'test_fail', testId: 'd.gctf#[row=1 dataset.id=2]' });
    expect(s.passed).toBe(1);
    expect(s.failed).toBe(1);
    expect(s.done).toBe(2);
  });

  it('leaves a file that is one case alone', () => {
    let s = emptyRun();
    s = applyEvent(s, { event: 'test_pass', testId: 'p.gctf', duration: 4 });
    expect(s.verdicts['p.gctf']).toEqual({ path: 'p.gctf', state: 'pass', durationMs: 4, message: undefined, assertions: undefined, documents: undefined, response: undefined });
  });
});

describe('what a scope runs', () => {
  it('runs the open file even when the rail is filtered past it', () => {
    expect(scopeFiles(['b.gctf'], 'file', 'a.gctf')).toEqual(['a.gctf']);
  });

  it('leaves folder and everything to the rail', () => {
    expect(scopeFiles(['x/b.gctf'], 'folder', 'x/a.gctf')).toEqual(['x/b.gctf']);
    expect(scopeFiles(['x/b.gctf'], 'all', 'x/a.gctf')).toEqual(['x/b.gctf']);
  });

  it('has nothing to run without a file', () => {
    expect(scopeFiles(['a.gctf'], 'file', null)).toEqual([]);
  });
});

describe('how long a file took, in a rail 15rem wide', () => {
  it('is said the way the rest of the workbench says it', () => {
    expect(verdictLabel({ path: 'a.gctf', state: 'pass', durationMs: 20006 })).toBe('20 s');
    expect(verdictLabel({ path: 'a.gctf', state: 'pass', durationMs: 1500 })).toBe('1.5 s');
    expect(verdictLabel({ path: 'a.gctf', state: 'pass', durationMs: 42 })).toBe('42 ms');
  });

  it('still says passed when nothing was timed', () => {
    expect(verdictLabel({ path: 'a.gctf', state: 'pass' })).toBe('passed');
  });
});

describe('unsavedAmong', () => {
  const tab = (path: string | null, dirty: boolean) => ({ path, dirty });

  it('names the targets an open tab has edits for', () => {
    expect(unsavedAmong(['a.gctf', 'b.gctf'], [tab('a.gctf', true), tab('b.gctf', false)]))
      .toEqual(['a.gctf']);
  });

  it('ignores edits to files this run is not reading', () => {
    expect(unsavedAmong(['a.gctf'], [tab('c.gctf', true)])).toEqual([]);
  });

  it('ignores a tab with no file behind it', () => {
    expect(unsavedAmong(['a.gctf'], [tab(null, true)])).toEqual([]);
  });

  it('counts one file once, however many tabs hold it', () => {
    expect(unsavedAmong(['a.gctf'], [tab('a.gctf', true), tab('a.gctf', true)])).toEqual(['a.gctf']);
  });
});

describe('a run that stopped where it was told to', () => {
  const passed = { path: 'chain.httf', state: 'pass' as const, durationMs: 5 };

  it('says how far it got rather than calling the file passed', () => {
    expect(verdictLabel(passed, 2)).toBe('steps 1–2');
    expect(verdictLabel(passed, 1)).toBe('step 1');
  });

  it('reads as a whole-file verdict when the whole file ran', () => {
    expect(verdictLabel(passed)).toBe('5 ms');
  });

  it('still says a file that did not run did not run', () => {
    expect(verdictLabel({ path: 'a.gctf', state: 'skip' }, 2)).toBe('not run');
  });
});

describe('the answer of a file driven by rows', () => {
  it('carries the case it came from', () => {
    let run = emptyRun();
    run = applyEvent(run, { event: 'test_pass', testId: 'rows.httf#[row=0 host=a]', duration: 1 } as never);
    run = applyEvent(run, {
      event: 'test_fail', testId: 'rows.httf#[row=1 host=b]', duration: 2,
      response: { messages: [], headers: {}, trailers: {}, error: 'nope' },
    } as never);

    const verdict = run.verdicts['rows.httf'];
    expect(verdict.caseLabel).toBe('row 2 of 2 · host=b');
    expect(verdictResult(verdict)?.fromCase).toBe('row 2 of 2 · host=b');
  });

  it('says nothing about a case for a file that runs once', () => {
    const one = { path: 'a.gctf', state: 'fail' as const, durationMs: 1 };
    expect(verdictResult(one)?.fromCase).toBeUndefined();
  });
});

describe('which step the kept answer belongs to', () => {
  it('carries the step the run reported', () => {
    const s = applyEvent(emptyRun(), {
      event: 'test_fail',
      testId: 'chain.gctf',
      duration: 9,
      documents: [3, 2],
      response: { messages: [{ message: 'hi' }] },
      responseStep: 1,
    });
    expect(verdictResult(s.verdicts['chain.gctf'])?.fromStep).toBe(1);
  });

  it('carries the step of a passing chain too', () => {
    const s = applyEvent(emptyRun(), {
      event: 'test_pass',
      testId: 'chain.apif',
      duration: 9,
      documents: [3, 2],
      responseStep: 1,
    });
    expect(verdictResult(s.verdicts['chain.apif'])?.fromStep).toBe(1);
  });

  it('says nothing for a file of one document', () => {
    const s = applyEvent(emptyRun(), {
      event: 'test_fail',
      testId: 'one.gctf',
      duration: 4,
      response: { messages: [] },
    });
    expect(verdictResult(s.verdicts['one.gctf'])?.fromStep).toBeUndefined();
  });
});

describe('more than one row failing', () => {
  it('says the line is one of them', () => {
    expect(moreRowsNote({ path: 'a.httf', state: 'fail', cases: { total: 3, failed: 2 } })).toBe('+1 more row failed');
    expect(moreRowsNote({ path: 'a.httf', state: 'fail', cases: { total: 3, failed: 3 } })).toBe('+2 more rows failed');
  });

  it('says nothing when one row failed, or none', () => {
    expect(moreRowsNote({ path: 'a.httf', state: 'fail', cases: { total: 2, failed: 1 } })).toBe(null);
    expect(moreRowsNote({ path: 'a.httf', state: 'fail' })).toBe(null);
  });
});

describe('where a run went', () => {
  it('travels with the verdict', () => {
    const run = applyEvent(emptyRun(), {
      event: 'test_pass',
      testId: 'a.gctf',
      duration: 4,
      address: 'localhost:50051',
    } as never);
    expect(run.verdicts['a.gctf'].address).toBe('localhost:50051');
  });
});

describe('a bench that took no measurement', () => {
  it('says why', () => {
    const run = applyEvent(applyEvent(emptyRun(), { event: 'suite_start', testCount: 1 } as never), {
      event: 'test_fail',
      testId: '40 files',
      message: 'invalid test document(s):\n  a.gctf: Validation failed',
      duration: 3,
    } as never);
    expect(benchFailure({ ...run, kind: 'bench' })).toContain('invalid test document(s)');
  });

  it('says nothing about a run, or about a bench that produced numbers', () => {
    const run = applyEvent(emptyRun(), { event: 'test_fail', testId: 'a.gctf', message: 'nope' } as never);
    expect(benchFailure(run)).toBeNull();
    expect(benchFailure({ ...run, kind: 'bench', benchReport: { summary: {} } as never })).toBeNull();
  });
});

describe('the headline of a refused file', () => {
  it('reads the reason, not the heading over it', () => {
    expect(failureHeadline('Validation error: Validation failed:\nAt least one verification section (RESPONSE, ERROR, or ASSERTS) is required'))
      .toBe('Validation error: At least one verification section (RESPONSE, ERROR, or ASSERTS) is required');
  });

  it('keeps a line number that means a line', () => {
    expect(failureHeadline('Validation error: Validation failed:\nLine 7: Unknown OPTIONS key'))
      .toBe('Validation error: Line 7: Unknown OPTIONS key');
  });

  it('still prefers a bullet, the way the runner writes one', () => {
    expect(failureHeadline('Failed:\n- .id == "x" did not hold')).toBe('.id == "x" did not hold');
  });

  it('leaves a one-line message alone', () => {
    expect(failureHeadline('Connection refused')).toBe('Connection refused');
  });
});

describe('a duration worth putting on the row', () => {
  it('is nothing under a second — forty of those are noise', () => {
    expect(slowNote(20)).toBeNull();
    expect(slowNote(999)).toBeNull();
    expect(slowNote(undefined)).toBeNull();
  });

  it('is the time itself once a test costs some', () => {
    expect(slowNote(1000)).toBe('1.0 s');
    expect(slowNote(4200)).toBe('4.2 s');
  });

  it('takes a floor of its own where a caller has one', () => {
    expect(slowNote(400, 250)).toBe('400 ms');
  });
});

describe('what a run covered', () => {
  it('is nothing to say when no file had a schema', () => {
    expect(coverageNote(undefined)).toBeNull();
    expect(coverageNote({ covered: 0, methods: 0, untested: [] })).toBeNull();
  });

  it('counts the methods and names the ones nothing called', () => {
    const note = coverageNote({
      covered: 1,
      methods: 3,
      untested: ['grpc://EchoService/Echo', 'grpc://EchoService/Repeat'],
    });
    expect(note?.label).toBe('methods 1/3');
    expect(note?.title).toBe('Never called by this run:\nEchoService/Echo\nEchoService/Repeat');
  });

  it('stops listing where a title stops being read', () => {
    const untested = Array.from({ length: 12 }, (_, i) => `grpc://S/M${i}`);
    expect(coverageNote({ covered: 0, methods: 12, untested }, 3)?.title)
      .toBe('Never called by this run:\nS/M0\nS/M1\nS/M2\nand 9 more');
  });

  it('says so when everything was called', () => {
    expect(coverageNote({ covered: 3, methods: 3, untested: [] })?.title)
      .toBe('Every method of the schemas this run dialled was called');
  });
});

describe('what a run hands the panel', () => {
  it('keeps the request each check ran against', () => {
    const result = verdictResult({
      path: 'chain.httf',
      state: 'fail',
      assertions: [
        { line: 8, expression: '@status() == 200', passed: true, endpoint: 'GET /v1/users' },
        { line: 18, expression: '.name == "Grace"', passed: false, endpoint: 'GET /v1/users/7' },
      ],
    });
    expect(result?.assertions?.map(a => a.endpoint)).toEqual(['GET /v1/users', 'GET /v1/users/7']);
  });

  it('says nothing about it when the run did not', () => {
    const result = verdictResult({
      path: 'a.gctf',
      state: 'pass',
      assertions: [{ line: 3, expression: '.ok', passed: true }],
    });
    expect(result?.assertions?.[0].endpoint).toBeUndefined();
  });
});

describe('what the answer came back with', () => {
  it('travels from the event to the verdict', () => {
    const state = applyEvent(emptyRun(), {
      event: 'test_pass',
      testId: 'probe.httf',
      grpcStatus: 200,
    });
    expect(state.verdicts['probe.httf'].statusCode).toBe(200);
    expect(verdictResult(state.verdicts['probe.httf'])?.statusCode).toBe(200);
  });

  it('carries a zero rather than dropping it', () => {
    const state = applyEvent(emptyRun(), { event: 'test_pass', testId: 'a.gctf', grpcStatus: 0 });
    expect(verdictResult(state.verdicts['a.gctf'])?.statusCode).toBe(0);
  });

  it('says nothing when the run never got one', () => {
    const state = applyEvent(emptyRun(), { event: 'test_fail', testId: 'b.gctf', message: 'refused' });
    expect(verdictResult(state.verdicts['b.gctf'])?.statusCode).toBeNull();
  });
});

describe('what a check cost', () => {
  it('travels with the check', () => {
    const state = applyEvent(emptyRun(), {
      event: 'test_fail',
      testId: 'slow.gctf',
      assertions: [{ line: 4, expression: '@len(.items) > 0', passed: true, elapsedMs: 12 }],
    });
    expect(verdictResult(state.verdicts['slow.gctf'])?.assertions?.[0].elapsed_ms).toBe(12);
  });

  it('reads as instant when the run said nothing', () => {
    const state = applyEvent(emptyRun(), {
      event: 'test_pass',
      testId: 'quick.gctf',
      assertions: [{ line: 4, expression: '.ok', passed: true }],
    });
    expect(verdictResult(state.verdicts['quick.gctf'])?.assertions?.[0].elapsed_ms).toBe(0);
  });
});

describe('the samples a bench reports', () => {
  it('keeps them in order', () => {
    let s = applyEvent(emptyRun(), { event: 'bench_progress', elapsed_s: 1, rps: 100, targetRps: 200 });
    s = applyEvent(s, { event: 'bench_progress', elapsed_s: 2, rps: 180, targetRps: 200 });
    expect(s.benchTicks.map(t => t.rps)).toEqual([100, 180]);
    expect(s.benchProgress?.rps).toBe(180);
  });

  it('does not draw a replayed sample twice', () => {
    let s = applyEvent(emptyRun(), { event: 'bench_progress', elapsed_s: 1, rps: 100 });
    s = applyEvent(s, { event: 'bench_progress', elapsed_s: 2, rps: 180 });
    s = applyEvent(s, { event: 'bench_progress', elapsed_s: 1, rps: 100 });
    s = applyEvent(s, { event: 'bench_progress', elapsed_s: 2, rps: 180 });
    expect(s.benchTicks.map(t => t.elapsed_s)).toEqual([1, 2]);
  });
});

describe('a refused run in the workbench\'s words', () => {
  it('offers Save when a tab still holds the file', () => {
    const said = runRefusal('File not found: api/probe.gctf', ['api/probe.gctf']);
    expect(said.path).toBe('api/probe.gctf');
    expect(said.text).toBe('api/probe.gctf is not on disk any more — Save writes this tab back to it');
  });

  it('says it went behind the rail when nothing holds it', () => {
    const said = runRefusal('File not found: api/probe.gctf', ['other.gctf']);
    expect(said.text).toBe('api/probe.gctf is not on disk any more — it was renamed or deleted since the rail read it');
  });

  it('leaves any other refusal alone', () => {
    expect(runRefusal('Data source not found: paths.csv', [])).toEqual({
      text: 'Data source not found: paths.csv',
      path: null,
    });
  });
});

describe('what the answer beside a step cost', () => {
  const chain = {
    path: 'checkout.apif', state: 'pass' as const, durationMs: 43,
    documents: [31, 12], responseStep: 1,
    response: { messages: [], headers: {}, trailers: {}, error: null },
  };

  it('is that step\'s duration, not the file\'s', () => {
    expect(verdictResult(chain)?.durationMs).toBe(12);
  });

  it('falls back to the file when the run reported no per-step durations', () => {
    expect(verdictResult({ ...chain, documents: undefined })?.durationMs).toBe(43);
  });

  it('keeps the file total when no step is named', () => {
    expect(verdictResult({ ...chain, responseStep: undefined })?.durationMs).toBe(43);
  });
});

describe('which row an answer belongs to', () => {
  it('counts from one and says how many there are', () => {
    expect(caseTitle('row=0', 2)).toBe('row 1 of 2');
  });

  it('keeps what the row was bound to', () => {
    expect(caseTitle('row=0 dataset.who=World', 2)).toBe('row 1 of 2 · dataset.who=World');
  });

  it('says the number alone when nothing said how many', () => {
    expect(caseTitle('row=3 who=a')).toBe('row 4 · who=a');
  });

  it('has nothing to say about a file that has no rows', () => {
    expect(caseTitle(null)).toBe(null);
    expect(caseTitle('')).toBe(null);
  });

  it('passes an id it cannot read through', () => {
    expect(caseTitle('case=7')).toBe('case=7');
  });
});

describe('what the run counts are counting', () => {
  const run = (over: Partial<RunState>): RunState => ({ ...emptyRun(), ...over });

  it('says nothing when every file ran once', () => {
    expect(caseNote(run({ total: 2, verdicts: { 'a.gctf': {} as never, 'b.gctf': {} as never } }))).toBeNull();
  });

  it('says both numbers when a file ran more than once', () => {
    const said = caseNote(run({
      total: 26,
      verdicts: Object.fromEntries(Array.from({ length: 25 }, (_, i) => [`f${i}.gctf`, {} as never])),
    }));
    expect(said?.label).toBe('26 cases · 25 files');
    expect(said?.title).toContain('rows');
  });

  it('says nothing before the run has reported a file', () => {
    expect(caseNote(run({ total: 26, verdicts: {} }))).toBeNull();
  });
});

describe('what a bench in flight says', () => {
  const benching = (over: Partial<RunState>): RunState => ({ ...emptyRun(), kind: 'bench', ...over });

  it('says nothing about a test run', () => {
    expect(benchLine({ ...emptyRun(), kind: 'run' })).toBeNull();
  });

  it('says it is starting before the first sample', () => {
    expect(benchLine(benching({}))?.label).toBe('starting');
  });

  it('reads elapsed, requests and rate', () => {
    const said = benchLine(benching({
      benchProgress: { elapsed_s: 3.4, requests: 1112, errors: 0, rps: 371.2, targetRps: 400, errorPct: 0 },
    }));
    expect(said?.label).toBe('3 s · 1112 req · 371 rps');
    expect(said?.title).toContain('400 rps');
  });

  it('adds the error rate only when there is one', () => {
    const said = benchLine(benching({
      benchProgress: { elapsed_s: 10, requests: 100, errors: 3, rps: 10, targetRps: 0, errorPct: 3 },
    }));
    expect(said?.label).toBe('10 s · 100 req · 10 rps · 3.00% err');
    expect(said?.title).toBe('3 of 100 came back an error');
  });
});
