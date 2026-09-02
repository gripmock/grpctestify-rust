import { useCallback, useEffect, useMemo, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { useStore } from '../../lib/store';
import { stepPhrase } from '../../lib/assert-line';
import { useKept } from '../../lib/tool-scratch';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import { loadRecent, pushRecent, saveRecent } from '../../lib/history-list';
import { byteSize, humanBytes } from '../../lib/format';
import { collectPaths } from '../../lib/json-paths';
import { copyToClipboard } from 'luvo/data/clipboard';
import { useToast } from 'luvo/ui/useToast';
import { acrossStream, streamNote } from '../../lib/pick-actions';
import { Copy, Wand2, ChevronDown, History, Braces } from 'lucide-react';
import { count } from 'luvo/data/plural';

type QueryOut = { outputs: unknown[]; error: string | null; elapsed_us: number };

function elapsed(us: number): string {
  return us < 1000 ? `${us} µs` : `${(us / 1000).toFixed(1)} ms`;
}
type Snippet = { label: string; detail: string | null };
type Source = 'response' | 'request' | 'pasted';

const RECENT_KEY = 'play.jq.recent';

function isPath(filter: string): boolean {
  return /^\.[A-Za-z0-9_.[\]"]*$/.test(filter);
}

export function JqTester({ seed, messages = [], handed = null }: {
  seed: unknown | null;
  messages?: unknown[];
  handed?: { expr: string; n: number } | null;
}) {
  const steps = useStore(s => s.documents.length);
  const activeStep = useStore(s => s.activeStep);
  const where = stepPhrase(steps, activeStep);

  const toast = useToast();
  const addExtract = useStore(s => s.addExtract);
  const addAssert = useStore(s => s.addAssert);
  const requestBodies = useStore(s => s.request.bodies);

  const [source, setSource] = useKept<Source>('jq.source', () => (seed !== null ? 'response' : 'pasted'));
  const [pasted, setPasted] = useKept('jq.pasted', () => '{\n  "example": true\n}');
  const [expr, setExpr] = useKept('jq.expr', () => '.');
  const [runs, setRuns] = useState(0);
  const [recent, setRecent] = useState<string[]>(() => loadRecent(RECENT_KEY));
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [menu, setMenu] = useState<'snippets' | 'history' | null>(null);
  const menuRef = useDismiss<HTMLDivElement>(menu !== null, useCallback(() => setMenu(null), [setMenu]));
  const [assertKind, setAssertKind] = useKept<'equals' | 'present'>('jq.assertKind', () => 'equals');

  const hasSeed = seed !== null;
  const handedN = handed?.n ?? 0;
  const handedExpr = handed?.expr ?? '';
  useEffect(() => {
    if (handedN === 0) return;
    setExpr(handedExpr);
    if (hasSeed) setSource('response');
  }, [handedN, handedExpr, hasSeed, setExpr, setSource]);

  useEffect(() => {
    let live = true;
    fetch('/api/snippets').then(r => (r.ok ? r.json() : [])).then(d => { if (live) setSnippets(d); }).catch(() => {});
    return () => { live = false; };
  }, []);

  const requestBody = requestBodies[0]?.trim() || null;
  const whose = where === '' ? 'this file' : `this ${where}`;
  const named = where === '' ? 'This file' : `${where[0].toUpperCase()}${where.slice(1)}`;
  const live: Source =
    source === 'response' && seed === null ? 'pasted'
    : source === 'request' && requestBody === null ? 'pasted'
    : source;
  const sourceText =
    live === 'response' ? JSON.stringify(seed, null, 2)
    : live === 'request' ? requestBody!
    : pasted;

  let parsed: unknown = null;
  let parseError: string | null = null;
  try { parsed = JSON.parse(sourceText); } catch (e: any) { parseError = e?.message ?? 'invalid JSON'; }

  const { data, error, busy } = useDebouncedPost<QueryOut>(
    '/api/eval/query',
    parseError === null && expr.trim() ? { input: parsed, expr: expr.trim(), runs } : null,
  );

  const outputs = useMemo(() => data?.outputs ?? [], [data]);
  const failure = error ?? data?.error ?? null;
  const paths = useMemo(() => (parseError === null ? collectPaths(parsed, 6) : []), [parsed, parseError]);
  const suggestedName = useMemo(
    () => expr.trim().split(/[.[]/).filter(Boolean).pop()?.replace(/\W/g, '') || 'value',
    [expr],
  );

  const assertion = useMemo(() => {
    const filter = expr.trim();
    if (!filter) return '';
    if (assertKind === 'present') return `@has_value(${filter})`;
    if (outputs.length !== 1) return '';
    return isPath(filter)
      ? `${filter} == ${JSON.stringify(outputs[0])}`
      : `(${filter}) == ${JSON.stringify(outputs[0])}`;
  }, [expr, assertKind, outputs]);

  const noAssertion = useMemo(() => {
    if (!expr.trim()) return 'The filter is empty';
    if (failure) return 'The filter has to run first';
    if (!data) return 'Evaluating…';
    if (assertKind === 'equals' && outputs.length !== 1)
      return `${outputs.length} outputs — an equality needs exactly one; "is present" takes any`;
    if (assertKind === 'equals' && isPath(expr.trim())) {
      const note = streamNote(acrossStream(messages, expr.trim(), outputs[0]), messages.length);
      if (note) return note;
    }
    return null;
  }, [expr, failure, data, assertKind, outputs, messages]);

  const commit = () => {
    const next = pushRecent(recent, expr);
    setRecent(next);
    saveRecent(RECENT_KEY, next);
    setRuns(n => n + 1);
  };

  const append = (fragment: string) => {
    setExpr(v => (v.trim() && !v.trimEnd().endsWith('|') ? `${v} | ${fragment}` : `${v}${fragment}`));
    setMenu(null);
  };

  return (
    <div className="stack">
      <div className="stack is-tight">
        <div className="bar" ref={menuRef}>
          <span className="field-label grow">filter</span>
          <div className="picker">
            <button
              className="btn is-sm is-ghost"
              onClick={() => setMenu(m => (m === 'snippets' ? null : 'snippets'))}
              aria-haspopup="menu"
              aria-expanded={menu === 'snippets'}
              disabled={snippets.length === 0}
              title="jq fragments to append to the filter"
            >
              <Wand2 size={11} /> snippets <ChevronDown size={10} />
            </button>
            <Popover open={menu === 'snippets'} anchor={menuRef} align="end" className="tool-menu">
              <div className="menu">
                {snippets.map(s => (
                  <button key={s.label} className="menu-item mono" onClick={() => append(s.label)}>
                    {s.label}
                    {s.detail && <span className="muted"> — {s.detail}</span>}
                  </button>
                ))}
              </div>
            </Popover>
          </div>
          <div className="picker">
            <button
              className="btn is-sm is-ghost"
              disabled={recent.length === 0}
              onClick={() => setMenu(m => (m === 'history' ? null : 'history'))}
              aria-haspopup="menu"
              aria-expanded={menu === 'history'}
              title={recent.length === 0 ? 'Filters you run are remembered here' : `${count(recent.length, 'filter')} this browser has run`}
            >
              <History size={11} /> history <ChevronDown size={10} />
            </button>
            <Popover open={menu === 'history'} anchor={menuRef} align="end" className="tool-menu">
              <div className="menu">
                {recent.map(r => (
                  <button key={r} className="menu-item mono" onClick={() => { setExpr(r); setMenu(null); }}>{r}</button>
                ))}
                <div className="menu-sep" />
                <button
                  className="menu-item"
                  onClick={() => { setRecent([]); saveRecent(RECENT_KEY, []); setMenu(null); }}
                >
                  Forget these
                </button>
              </div>
            </Popover>
          </div>
        </div>

        <div className={`field-frame expr-frame${failure ? ' is-bad' : ''}`}>
          <span className="expr-gutter mono">jq</span>
          <input
            className="field mono"
            spellCheck={false}
            placeholder=".items | map(.id) · to_entries · length"
            value={expr}
            onChange={e => setExpr(e.target.value)}
            onBlur={commit}
            onKeyDown={e => { if (e.key === 'Enter') commit(); }}
          />
        </div>
      </div>

      <div className="tool-grid">
        <div className="stack is-cell">
          <div className="bar">
            <span className="field-label grow">input</span>
            <span className="badge">{humanBytes(byteSize(sourceText))}</span>
            <button
              className="btn is-sm is-ghost"
              disabled={live !== 'pasted' || parseError !== null}
              onClick={() => setPasted(JSON.stringify(JSON.parse(pasted), null, 2))}
              title={
                live !== 'pasted' ? 'Only the pasted input is editable'
                : parseError !== null ? 'Not JSON yet — nothing to indent'
                : 'Pretty-print the pasted JSON'
              }
            >
              <Braces size={11} /> format
            </button>
            <Seg
              label="What the filter reads"
              value={live}
              onChange={setSource}
              options={[
                { value: 'response', label: 'last response', disabled: seed === null, title: seed === null ? 'Nothing has been executed yet' : 'The messages the last call returned' },
                { value: 'request', label: `${whose}'s request`, disabled: requestBody === null, title: requestBody === null ? `${named} has no REQUEST yet` : `The message ${where === '' ? 'this file' : where} would send` },
                { value: 'pasted', label: 'pasted', title: 'Any JSON typed below' },
              ]}
            />
          </div>
          <textarea
            className="field field-frame code-input tool-input"
            spellCheck={false}
            readOnly={live !== 'pasted'}
            value={sourceText}
            onChange={e => setPasted(e.target.value)}
          />
          {parseError && <div className="assert is-fail"><span className="assert-mark">!</span><span>{parseError}</span></div>}
          {paths.length > 0 && (
            <div className="bar wrap tool-paths">
              <span className="field-label">paths</span>
              {paths.map(p => (
                <button key={p} className="chip mono" onClick={() => setExpr(p)}>{p}</button>
              ))}
            </div>
          )}
        </div>

        <div className="stack is-cell">
          <div className="bar">
            <span className="field-label grow">output</span>
            {busy && <span className="muted">…</span>}
            {!failure && data && (
              <>
                <span className="badge">{count(outputs.length, 'output')}</span>
                <span className="muted mono">{elapsed(data.elapsed_us)}</span>
              </>
            )}
            <button
              className="btn is-sm is-ghost"
              disabled={outputs.length === 0}
              title={outputs.length === 0 ? 'The filter produced nothing to copy' : 'Copy every output'}
              onClick={async () => {
                const text = outputs.map(o => JSON.stringify(o, null, 2)).join('\n');
                try {
                  await copyToClipboard(text);
                  toast.success(`${count(outputs.length, 'output')} copied`);
                } catch {
                  toast.error('The browser refused the clipboard');
                }
              }}
            >
              <Copy size={11} /> copy
            </button>
          </div>

          {failure && <div className="assert is-fail"><span className="assert-mark">!</span><span>{failure}</span></div>}

          {!failure && (
            <pre className="diff tool-out">
              {outputs.length === 0 ? '' : outputs.map(o => JSON.stringify(o, null, 2)).join('\n')}
            </pre>
          )}

          {!failure && data && outputs.length === 0 && (
            <div className="note">No output. In an assertion that is falsey — a filter that emits nothing fails.</div>
          )}

        </div>
      </div>

      <div className="assert-preview">
        <div className="bar">
          <Seg
            label="What the assertion says"
            value={assertKind}
            onChange={setAssertKind}
            options={[
              { value: 'equals', label: 'equals the value', title: 'Pins the value the filter produced — fails if it changes' },
              { value: 'present', label: 'is present', title: '@has_value — passes on any non-empty value' },
            ]}
          />
          <span className="grow" />
          <button
            className="btn is-sm is-ghost"
            disabled={!expr.trim() || failure !== null}
            onClick={() => {
              addExtract(suggestedName, expr.trim());
              commit();
              toast.success(`{{${suggestedName}}} = ${expr.trim()} — rename it in EXTRACT`);
            }}
            title={
              !expr.trim() ? 'The filter is empty'
              : failure ? 'The filter has to run first'
              : `Writes EXTRACT into ${where || 'the open file'} — binds as {{${suggestedName}}}`
            }
          >
            to EXTRACT
          </button>
          <button
            className="btn is-sm"
            disabled={noAssertion !== null}
            onClick={() => {
              const said = addAssert(assertion);
              commit();
              if (said === 'duplicate') toast.info('This file already asserts that');
              else toast.success(`Assertion added${where ? ` to ${where}` : ''} — Save writes it to the file`);
            }}
            title={noAssertion ?? `Writes this line into ${where ? `${where}'s` : "the open file's"} ASSERTS`}
          >
            add to ASSERTS
          </button>
        </div>
        <pre className={`diff assert-line${assertion ? '' : ' muted'}${noAssertion && assertion ? ' is-refused' : ''}`}>
          {assertion || noAssertion || '—'}
        </pre>
        {noAssertion && assertion && <div className="note is-warn assert-refused">{noAssertion}</div>}
      </div>
    </div>
  );
}
