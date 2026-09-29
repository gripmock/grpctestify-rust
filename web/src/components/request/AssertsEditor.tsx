import { useMemo, useRef, useState } from 'react';
import { useStore } from '../../lib/store';
import { assertWhy } from '../../lib/assert-line';
import { sectionRun } from '../../lib/message-attributes';
import { problemsFor } from '../../lib/assert-problems';
import { sectionAsWritten } from '../../lib/body-as-written';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import { Check, X, TriangleAlert, RefreshCw, Wand2, Pencil } from 'lucide-react';
import { answered } from '../../lib/response-seed';

type Verdict = {
  expression: string;
  layer: 'ast' | 'jq' | null;
  passed: boolean;
  error: string | null;
  message: string | null;
  expected: string | null;
  actual: string | null;
  hint?: string | null;
  suggestion: string | null;
  elapsed_us: number;
};

export function AssertsEditor({ asserts }: { asserts: string[] }) {
  const addAssert = useStore(s => s.addAssert);
  const removeAssert = useStore(s => s.removeAssert);
  const replaceAssert = useStore(s => s.replaceAssert);
  const response = useStore(s => s.response);
  const [draft, setDraft] = useState('');
  const [checkedAt, setCheckedAt] = useState(0);
  const [editing, setEditing] = useState<number | null>(null);
  const [editDraft, setEditDraft] = useState('');
  const editRef = useRef<HTMLInputElement>(null);

  const activeStep = useStore(s => s.activeStep);
  const steps = useStore(s => s.documents.length);
  const answerElsewhere = steps > 1
    && response?.fromStep !== undefined
    && response.fromStep !== activeStep
    ? response.fromStep
    : null;
  const lastMessage = answered(response) && answerElsewhere === null
    ? response!.messages?.[0]
    : undefined;
  const canCheck = lastMessage !== undefined && asserts.length > 0;
  const skipped = useStore(s => sectionRun(s.collectionParsed, 'ASSERTS').skipped);
  const diagnostics = useStore(s => s.diagnostics);
  const written = useStore(s => sectionAsWritten(s.collectionParsed, 'ASSERTS'));

  const trimmedDraft = draft.trim();
  const expressions = trimmedDraft && !asserts.includes(trimmedDraft)
    ? [...asserts, trimmedDraft]
    : asserts;

  const { data, busy } = useDebouncedPost<Verdict[]>(
    '/api/eval/assert',
    lastMessage !== undefined && expressions.length > 0
      ? {
          response: lastMessage,
          headers: response?.headers ?? {},
          trailers: response?.trailers ?? {},
          elapsed_ms: response?.durationMs ?? 0,
          expressions,
          nonce: checkedAt,
        }
      : null,
  );

  const byExpr = new Map((data ?? []).map(v => [v.expression, v]));
  const saved = asserts.map(a => byExpr.get(a.trim())).filter((v): v is Verdict => v != null);
  const passed = saved.filter(v => v.passed).length;
  const failed = saved.length - passed;
  const draftVerdict = trimmedDraft ? byExpr.get(trimmedDraft) : undefined;
  const duplicate = trimmedDraft !== '' && asserts.some(a => a.trim() === trimmedDraft);

  const submit = () => {
    if (!trimmedDraft || duplicate) return;
    addAssert(draft);
    setDraft('');
  };

  const commitEdit = (index: number) => {
    const next = editDraft.trim();
    setEditing(null);
    if (next && next !== asserts[index].trim()) replaceAssert(index, next);
  };

  const inert = useMemo(() => {
    const found = new Set<string>();
    for (const line of [...asserts, trimmedDraft]) {
      for (const match of (line ?? '').matchAll(/\{\{([^{}]*)\}\}/g)) {
        const name = match[1].trim();
        if (name !== '') found.add(name);
      }
    }
    return [...found] as string[];
  }, [asserts, trimmedDraft]);

  return (
    <div className="stack">
      {skipped && (
        <div className="note is-warn">
          <span className="mono">#[skip]</span> on <span className="mono">ASSERTS</span> — a run walks
          past these and checks nothing. A file whose every check is skipped is refused by
          <span className="mono"> check</span>, the way a file with no check at all is. They still
          evaluate here against the last response.
        </div>
      )}
      {written !== null && (
        <div className="note" title={written}>
          The file writes these with comments — editing a line here saves the section without them.
        </div>
      )}
      {inert.length > 0 && (
        <div className="note is-warn">
          {inert.map(n => `{{${n}}}`).join(', ')} — nothing is substituted in ASSERTS: the expression
          runs as written and compares against the braces themselves. Substitution reaches the
          request, its headers and the expected response.
        </div>
      )}
      <div className="bar">
        {canCheck && data && (
          <>
            {passed > 0 && <span className="badge is-ok">{passed} pass</span>}
            {failed > 0 && <span className="badge is-fail">{failed} fail</span>}
          </>
        )}
        {!canCheck && asserts.length > 0 && (
          <span className="muted">
            {answerElsewhere === null
              ? 'Execute the request to check these against a real response'
              : `The answer on screen is step ${answerElsewhere + 1}'s — execute this step to check these`}
          </span>
        )}
        <span className="grow" />
        {asserts.length > 0 && (
          <button
            className="btn is-sm is-ghost"
            onClick={() => setCheckedAt(Date.now())}
            disabled={!canCheck || busy}
            title={
              lastMessage === undefined ? 'Nothing has been executed yet'
              : busy ? 'Checking…'
              : 'Evaluate every assertion against the last response, without calling the server again'
            }
          >
            <RefreshCw size={11} /> re-check against last response
          </button>
        )}
      </div>

      {asserts.length === 0 && (
        <div className="empty-state">No assertions — click a response field, or add one below</div>
      )}

      {asserts.map((a, i) => {
        const v = byExpr.get(a.trim());
        const refused = problemsFor(a, diagnostics);
        const state = refused.length > 0 ? ' is-fail'
          : v == null ? '' : v.passed ? ' is-ok' : ' is-fail';
        return (
          <div key={`${i}-${a}`} className={`assert${state}`}>
            <span className="assert-mark">
              {refused.length > 0 ? <TriangleAlert size={12} />
                : v == null ? '·' : v.passed ? <Check size={12} /> : v.error ? <TriangleAlert size={12} /> : <X size={12} />}
            </span>
            <span className="stack is-cell">
              <span className="bar">
                {editing === i ? (
                  <input
                    ref={editRef}
                    className="field field-frame mono grow"
                    value={editDraft}
                    autoFocus
                    spellCheck={false}
                    onChange={e => setEditDraft(e.target.value)}
                    onBlur={() => commitEdit(i)}
                    onKeyDown={e => {
                      if (e.key === 'Enter') { e.preventDefault(); commitEdit(i); }
                      if (e.key === 'Escape') { e.preventDefault(); setEditing(null); }
                    }}
                  />
                ) : (
                  <button
                    className="mono grow assert-expr"
                    onClick={() => { setEditing(i); setEditDraft(a); }}
                    title="Edit this line"
                  >
                    {a}
                    <Pencil size={10} className="assert-pencil" />
                  </button>
                )}
                {v?.layer && (
                  <span
                    className="badge"
                    title={v.layer === 'ast'
                      ? 'Fast path — $var and context plugins work here'
                      : 'jq fallback — $var does not resolve, context plugins are rejected'}
                  >
                    {v.layer}
                  </span>
                )}
                {v != null && v.elapsed_us > 0 && <span className="muted mono">{v.elapsed_us} µs</span>}
                <button className="btn is-ghost is-icon assert-remove" aria-label="Remove assertion" onClick={() => removeAssert(i)}>
                  <X size={11} />
                </button>
              </span>
              {v?.suggestion && (
                <span className="assert-hint">
                  <Wand2 size={10} />
                  <span className="mono grow">{v.suggestion}</span>
                  <button
                    className="btn is-sm is-ghost"
                    onClick={() => replaceAssert(i, v.suggestion!)}
                    title="fmt would rewrite this line the same way"
                  >
                    apply
                  </button>
                </span>
              )}
              {refused.map((problem, n) => (
                <span key={n} className="assert-said">{problem.message}</span>
              ))}
              {refused.length === 0 && v && !v.passed && <Why verdict={v} />}
            </span>
          </div>
        );
      })}

      <div className={`field-frame${draftVerdict && !draftVerdict.passed ? ' is-bad' : ''}`}>
        <input
          className="field mono"
          placeholder='.status == "ok" · @len(.items) > 0 · @is_uuid(.id)'
          value={draft}
          spellCheck={false}
          onChange={e => setDraft(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') submit(); }}
        />
        {draftVerdict && (
          <span className={`badge ${draftVerdict.passed ? 'is-ok' : 'is-fail'}`}>
            {draftVerdict.passed ? 'passes' : draftVerdict.error ? 'error' : 'fails'}
          </span>
        )}
        <button
          className="btn is-ghost is-sm"
          onClick={submit}
          disabled={!trimmedDraft || duplicate}
          title={duplicate ? 'This line is already in ASSERTS' : 'Add this line to ASSERTS'}
        >
          Add
        </button>
      </div>

      {draftVerdict && !draftVerdict.passed && <Why verdict={draftVerdict} />}
    </div>
  );
}

function Why({ verdict }: { verdict: Verdict }) {
  const why = assertWhy(verdict);
  if (!why && !verdict.error) return null;
  return (
    <span className="assert-why">
      {verdict.error && <span className="assert-said">{verdict.error}</span>}
      {why?.message && <span className="assert-said">{why.message}</span>}
      {why?.expected != null && (
        <><span className="assert-key">expected</span><span className="mono">{why.expected}</span></>
      )}
      {why?.actual != null && (
        <><span className="assert-key">actual</span><span className="mono">{why.actual}</span></>
      )}
      {why?.hint && <span className="assert-remedy">{why.hint}</span>}
    </span>
  );
}
