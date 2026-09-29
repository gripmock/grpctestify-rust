import { useMemo, useState, type CSSProperties } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { MonacoEditor as Editor } from '../MonacoEditor';
import { useStore } from '../../lib/store';
import { AssertsEditor } from './AssertsEditor';
import { EDITOR_THEME, registerMonaco } from '../../lib/monaco-theme';
import { GRPC_CODES, readErrorBody, writeErrorField } from '../../lib/grpc-codes';
import { disagreementNote, expectBody, expectDisagreement, expectMode } from '../../lib/expect-model';
import { jsonProblem, jsonStream } from '../../lib/format';
import { Plus, X, TriangleAlert, ArrowDownToLine, ChevronDown, ChevronRight } from 'lucide-react';
import type { CollectionParsed, ExpectMessage } from '../../lib/types';
import { isHttpRequest } from '../../lib/http-endpoint';
import { statusAssert, statusUnchecked } from '../../lib/http-expect';
import { callFailed } from '../../lib/call-outcome';
import { answered } from '../../lib/response-seed';
import { sectionRun } from '../../lib/message-attributes';

const MODES = [
  { key: 'none' as const, label: 'asserts only', hint: 'The asserts below are the whole check' },
  { key: 'response' as const, label: 'response', hint: 'The messages that must come back, compared field by field' },
  { key: 'error' as const, label: 'error', hint: 'The call must fail, with this status' },
];

const HTTP_HINT: Record<string, string> = {
  none: 'The asserts below are the whole check',
  response: 'The body that must come back',
};

function hintOf(key: string, isHttp: boolean): string {
  return (isHttp && HTTP_HINT[key]) || MODES.find(m => m.key === key)?.hint || '';
}

export function ExpectEditor({ parsed }: { parsed: CollectionParsed | null }) {
  const isHttp = useStore(s => isHttpRequest(s.workspacePath, s.request.endpoint));
  const addAssert = useStore(s => s.addAssert);
  const lastStatus = useStore(s => (s.response?.status === 'ok' || s.response?.status === 'error' ? s.response.statusCode : null));
  const setExpectMode = useStore(s => s.setExpectMode);
  const addExpectResponse = useStore(s => s.addExpectResponse);
  const removeExpectResponse = useStore(s => s.removeExpectResponse);
  const setExpectResponse = useStore(s => s.setExpectResponse);
  const setExpectError = useStore(s => s.setExpectError);
  const response = useStore(s => s.response);

  const mode = expectMode(parsed);
  const skipped = useStore(s => sectionRun(s.collectionParsed, mode === 'error' ? 'ERROR' : 'RESPONSE').skipped);
  const responses = parsed?.expect_responses ?? [];
  const error = parsed?.expect_error ?? null;

  const activeStep = useStore(s => s.activeStep);
  const steps = useStore(s => s.documents.length);
  const answerElsewhere = steps > 1
    && response?.fromStep !== undefined
    && response.fromStep !== activeStep;
  const live = answered(response) && !answerElsewhere ? response!.messages ?? [] : [];

  const disagrees = disagreementNote(expectDisagreement(
    mode,
    response && response.status !== 'pending'
      ? { failed: isHttp ? !!response.error : callFailed(response, false) }
      : null,
  ));

  return (
    <div className="stack expect">
      <div className="bar expect-modes">
        <span className="field-label">must come back with</span>
        <Seg
          label="What this step expects"
          value={mode}
          onChange={setExpectMode}
          options={MODES.filter(m => m.key !== 'error' || !isHttp || mode === 'error')
            .map(m => ({ value: m.key, label: m.label, title: hintOf(m.key, isHttp) }))}
        />
        <span className="grow" />
        <span className="muted">{hintOf(mode, isHttp)}</span>
      </div>

      {skipped && (
        <div className="note is-warn">
          <span className="mono">#[skip]</span> on <span className="mono">{mode === 'error' ? 'ERROR' : 'RESPONSE'}</span> —
          a run walks past it and checks nothing here. A file whose every check is skipped is refused
          by <span className="mono">check</span>, the way a file with no check at all is.
        </div>
      )}

      {disagrees && <div className="note is-warn expect-disagrees"><TriangleAlert size={11} /> {disagrees}</div>}

      {mode === 'response' && (
        <div className="stack">
          {responses.map((message, i) => (
            <ExpectMessageEditor
              key={i}
              index={i}
              message={message}
              count={responses.length}
              isHttp={isHttp}
              onPatch={patch => setExpectResponse(i, patch)}
              onRemove={() => removeExpectResponse(i)}
              takeFromLive={live[i] !== undefined
                ? () => setExpectResponse(i, { body: expectBody(live[i]) })
                : undefined}
            />
          ))}
          {!(isHttp && responses.length > 0) && (
            <div className="bar">
              <button className="btn is-sm is-ghost" onClick={addExpectResponse}>
                <Plus size={12} /> {isHttp ? 'expected body' : 'expected message'}
              </button>
              {responses.length > 1 && (
                <span className="muted">a message per streamed response, in order</span>
              )}
            </div>
          )}
        </div>
      )}

      {mode === 'error' && isHttp && (
        <div className="note is-warn">
          An HTTP call does not fail the way a gRPC call does: a 404 or a 500 is a response that
          arrived with that status. Check the code in ASSERTS — <span className="mono">@status() == 404</span>{' '}
          — and the body as the expected response.
          <button className="btn is-sm is-ghost" onClick={() => setExpectMode('response')}>
            expect a response
          </button>
        </div>
      )}

      {mode === 'error' && !isHttp && error && <ExpectErrorEditor error={error} onPatch={setExpectError} />}

      {isHttp && (parsed?.asserts?.length ?? 0) + responses.length > 0
        && statusUnchecked(parsed?.asserts ?? []) && (
        <div className="note">
          <span className="grow">
            Nothing here checks the status — this file passes on any answer that carries this body,
            a <span className="mono">500</span> included.
          </span>
          <button
            className="btn is-sm"
            onClick={() => addAssert(statusAssert(lastStatus))}
            title="Written into ASSERTS, where the status is checked"
          >
            {statusAssert(lastStatus)}
          </button>
        </div>
      )}

      <AssertsEditor asserts={parsed?.asserts ?? []} />
    </div>
  );
}

function ExpectMessageEditor({
  index, message, count, isHttp, onPatch, onRemove, takeFromLive,
}: {
  index: number;
  message: ExpectMessage;
  count: number;
  isHttp: boolean;
  onPatch: (patch: Partial<ExpectMessage>) => void;
  onRemove: () => void;
  takeFromLive?: () => void;
}) {
  const themeMode = useStore(s => s.themeMode);
  const stream = useMemo(() => jsonStream(message.body), [message.body]);
  const problem = isHttp ? null : stream.problem;
  const comparable = !isHttp || stream.problem === null;
  const [open, setOpen] = useState(index === 0 || count <= 2);
  const silent = message.body.trim() === '';

  return (
    <div className={`stack expect-message${open ? '' : ' is-shut'}`}>
      <div className="bar">
        <button
          className="btn is-ghost is-icon is-sm"
          onClick={() => setOpen(v => !v)}
          aria-expanded={open}
          aria-label={open ? 'Collapse this message' : 'Expand this message'}
          title={open ? 'Collapse' : 'Expand'}
        >
          {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        </button>
        <span className="field-label">{count > 1 ? `message #${index + 1}` : isHttp ? 'expected body' : 'expected response'}</span>
        {!isHttp && stream.messages > 1 && (
          <span className="badge mono" title="This section expects a stream — the messages must come back in this order">
            {stream.messages} messages
          </span>
        )}
        {!open && (
          <>
            <span className="mono muted expect-peek" title={message.body}>
              {silent ? 'no messages' : message.body.replace(/\s+/g, ' ').trim().slice(0, 80)}
            </span>
            {(message.partial || message.unordered_arrays || message.tolerance != null) && (
              <span className="muted expect-rules">
                {[
                  message.partial ? 'partial' : null,
                  message.unordered_arrays ? 'unordered' : null,
                  message.tolerance != null ? `±${message.tolerance}` : null,
                ].filter(Boolean).join(' · ')}
              </span>
            )}
          </>
        )}
        {open && comparable && <button
          className={`chip${message.partial ? ' is-on' : ''}`}
          onClick={() => onPatch({ partial: !message.partial })}
          aria-pressed={message.partial}
          title="Compare as a subset: fields not named here are not checked"
        >
          partial
        </button>}
        {open && comparable && <button
          className={`chip${message.unordered_arrays ? ' is-on' : ''}`}
          onClick={() => onPatch({ unordered_arrays: !message.unordered_arrays })}
          aria-pressed={message.unordered_arrays}
          title="Compare arrays without regard to order"
        >
          unordered arrays
        </button>}
        {open && comparable && <label className="bar tolerance" title="Numeric fields may differ by this much">
          <span className="muted">±</span>
          <input
            className="field mono is-narrow"
            value={message.tolerance ?? ''}
            placeholder="0"
            inputMode="decimal"
            onChange={e => {
              const raw = e.target.value.trim();
              const parsed = Number(raw);
              onPatch({ tolerance: raw === '' || Number.isNaN(parsed) ? null : parsed });
            }}
          />
        </label>}
        <span className="grow" />
        {takeFromLive && (
          <button className="btn is-sm is-ghost" onClick={takeFromLive} title="Use what the last call returned">
            <ArrowDownToLine size={12} /> from the answer
          </button>
        )}
        {!isHttp && !silent && (
          <button
            className="btn is-sm is-ghost"
            onClick={() => onPatch({ body: '' })}
            title="Expect no messages — the call passes only when it produced none"
          >
            no messages
          </button>
        )}
        <button className="btn is-sm is-ghost is-icon" onClick={onRemove} title="Remove this expected message" aria-label="Remove this expected message">
          <X size={12} />
        </button>
      </div>

      {open && silent && (
        <div className="note">
          Empty — the call must come back with no messages. One that answers fails, and the failure
          says what it sent. Remove the block to stop checking the answer at all.
        </div>
      )}

      {open && message.redact.length > 0 && (
        <div className="note">
          redacted before comparing: <span className="mono">{message.redact.join(', ')}</span>
        </div>
      )}

      {open && <div
        className="editor"
        style={{ ['--body-lines' as string]: String(Math.max(1, message.body.split('\n').length)) } as CSSProperties}
      >
        <Editor
          height="100%"
          language={isHttp ? textOrJson(message.body) : 'json'}
          value={message.body}
          onChange={v => onPatch({ body: v || '' })}
          theme={EDITOR_THEME}
          onMount={(_ed, monaco) => registerMonaco(monaco, themeMode)}
          options={{
            minimap: { enabled: false }, fontSize: 13,
            scrollBeyondLastLine: false, wordWrap: 'on',
            automaticLayout: true, lineNumbers: 'on', tabSize: 2,
            bracketPairColorization: { enabled: true },
          }}
        />
      </div>}

      {open && problem && (
        <div className="note is-warn">
          <TriangleAlert size={12} /> {problem}
        </div>
      )}
    </div>
  );
}

function textOrJson(body: string): string {
  return jsonProblem(body) === null ? 'json' : 'plaintext';
}

function ExpectErrorEditor({
  error, onPatch,
}: {
  error: ExpectMessage;
  onPatch: (patch: Partial<ExpectMessage>) => void;
}) {
  const themeMode = useStore(s => s.themeMode);
  const shape = useMemo(() => readErrorBody(error.body), [error.body]);
  const problem = useMemo(() => jsonProblem(error.body), [error.body]);

  return (
    <div className="stack expect-message">
      <div className="bar">
        <span className="field-label">status</span>
        <select
          className="field is-narrow"
          value={shape?.code ?? ''}
          onChange={e => onPatch({
            body: writeErrorField(error.body, 'code', e.target.value === '' ? null : Number(e.target.value)),
          })}
          title="The gRPC status the call must fail with"
        >
          <option value="">any code</option>
          {GRPC_CODES.map(c => (
            <option key={c.code} value={c.code}>{c.name} ({c.code})</option>
          ))}
        </select>
        <textarea
          className="field grow expect-error-text"
          rows={(shape?.message ?? '').includes('\n') ? 3 : 1}
          value={shape?.message ?? ''}
          placeholder="message the failure must carry (optional)"
          onChange={e => onPatch({ body: writeErrorField(error.body, 'message', e.target.value) })}
        />
        <button
          className={`chip${error.partial ? ' is-on' : ''}`}
          onClick={() => onPatch({ partial: !error.partial })}
          aria-pressed={error.partial}
          title="Compare as a subset: fields not named here are not checked"
        >
          partial
        </button>
      </div>

      {shape?.extra && (
        <div className="note is-warn">
          <TriangleAlert size={12} />
          This ERROR carries more than a numeric code and a message — edit it below; the fields above
          only write those two.
        </div>
      )}

      <div
        className="editor"
        style={{ ['--body-lines' as string]: String(Math.max(1, error.body.split('\n').length)) } as CSSProperties}
      >
        <Editor
          height="100%"
          language="json"
          value={error.body}
          onChange={v => onPatch({ body: v || '' })}
          theme={EDITOR_THEME}
          onMount={(_ed, monaco) => registerMonaco(monaco, themeMode)}
          options={{
            minimap: { enabled: false }, fontSize: 13,
            scrollBeyondLastLine: false, wordWrap: 'on',
            automaticLayout: true, lineNumbers: 'on', tabSize: 2,
          }}
        />
      </div>

      {problem && (
        <div className="note is-warn">
          <TriangleAlert size={12} /> {problem}
        </div>
      )}
    </div>
  );
}
