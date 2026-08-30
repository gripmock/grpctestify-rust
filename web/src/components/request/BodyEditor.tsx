import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { MonacoEditor as Editor } from '../MonacoEditor';
import { useStore } from '../../lib/store';
import { everyMessageSkipped, messageRun } from '../../lib/message-attributes';
import { bodyAsWritten } from '../../lib/body-as-written';
import { Plus, X, Layers, Send, Sparkles, Loader2, TriangleAlert, ChevronUp, ChevronDown, CopyPlus } from 'lucide-react';
import { byteSize, humanBytes, jsonProblem } from '../../lib/format';
import { EnvVarToolbar } from './EnvVarToolbar';
import { schemaRequest } from '../../lib/schema-request';
import { currentEnv, currentRuntime } from '../../lib/env-runtime';
import { registerEnvHoverProvider, addEnvDecorations } from '../../lib/monaco-env-hover';
import { EDITOR_THEME, registerMonaco } from '../../lib/monaco-theme';
import { isHttpRequest, splitEndpoint } from '../../lib/http-endpoint';
import { bodyWithoutAMethodForIt, contentTypeOf, declaredContentType } from '../../lib/http-body';
import { joinForm, splitForm, type QueryParam } from '../../lib/query';
import { PairRows } from './PairRows';
import { Popover } from 'luvo/ui/Popover';
import { useDismiss } from 'luvo/input/useDismiss';
import { count } from 'luvo/data/plural';

let jsonFormatterRegistered = false;

function ensureJsonFormatter(monaco: any) {
  if (jsonFormatterRegistered) return;
  jsonFormatterRegistered = true;
  monaco.languages.registerDocumentFormattingEditProvider('json', {
    provideDocumentFormattingEdits(model: any) {
      try {
        const text = model.getValue();
        const parsed = JSON.parse(text);
        const formatted = JSON.stringify(parsed, null, 2);
        if (formatted === text) return [];
        return [{
          range: model.getFullModelRange(),
          text: formatted,
        }];
      } catch {
        return [];
      }
    },
  });
}

export function BodyEditor() {
  const request = useStore(s => s.request);
  const setRequestBody = useStore(s => s.setRequestBody);
  const addRequestBody = useStore(s => s.addRequestBody);
  const removeRequestBody = useStore(s => s.removeRequestBody);
  const moveRequestBody = useStore(s => s.moveRequestBody);
  const duplicateRequestBody = useStore(s => s.duplicateRequestBody);
  const themeMode = useStore(s => s.themeMode);

  const activeEnv = useStore(s => {
    const ae = s.activeEnvironment;
    return ae ? s.environments.find(e => e.name === ae) : null;
  });

  const environments = useStore(s => s.environments);
  const collectionParsed = useStore(s => s.collectionParsed);
  const activeStep = useStore(s => s.activeStep);
  const repaintEnv = useRef<(() => void) | null>(null);
  useEffect(() => { repaintEnv.current?.(); }, [activeEnv, environments, collectionParsed, activeStep]);

  const reflectionMethods = useStore(s => s.reflectionMethods);
  const method = useMemo(
    () => reflectionMethods.find(m => m.fullName === request.endpoint),
    [reflectionMethods, request.endpoint]
  );

  const isMulti = request.bodies.length > 1;
  const streamed = useStore(s => (s.response?.messages.length ?? 0) > 1);
  const reported = useStore(s => s.response?.shape ?? null);
  const clientStreams = method
    ? method.clientStreaming
    : reported === 'client' || reported === 'duplex';
  const serverStreams = method
    ? method.serverStreaming
    : reported === 'server' || reported === 'duplex' || (reported === null && streamed);
  const shape =
    clientStreams && serverStreams ? 'Bidirectional streaming'
    : clientStreams ? 'Client streaming'
    : serverStreams ? 'Server streaming'
    : reported !== null || method ? 'Unary'
    : isMulti ? 'Client streaming'
    : 'Unary';
  const wireNote = !isMulti ? null
    : clientStreams ? `${request.bodies.length} messages, one stream`
    : (method || reported !== null) ? `sent as ${request.bodies.length} separate calls`
    : null;
  const shapeMismatch = !clientStreams && isMulti && (!!method || reported !== null);

  const [fillingIdxs, setFillingIdxs] = useState<Set<number>>(new Set());
  const [fillErrors, setFillErrors] = useState<Record<number, string>>({});

  const handleAutoFill = async (idx: number) => {
    if (!request.endpoint) return;
    setFillingIdxs(prev => new Set(prev).add(idx));
    setFillErrors(prev => { const { [idx]: _drop, ...rest } = prev; return rest; });
    try {
      const res = await fetch('/api/schema-fill', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(schemaRequest(useStore.getState())),
      }).catch(() => { throw new Error('The workbench could not be reached — the body was not filled'); });
      const data = await res.json();
      if (data.error) {
        setFillErrors(prev => ({ ...prev, [idx]: data.error }));
        return;
      }
      setRequestBody(idx, JSON.stringify(data.schema, null, 2));
    } catch (err: any) {
      setFillErrors(prev => ({ ...prev, [idx]: err?.message || String(err) }));
    } finally {
      setFillingIdxs(prev => { const n = new Set(prev); n.delete(idx); return n; });
    }
  };

  const single = request.bodies.length === 1;
  const workspacePath = useStore(s => s.workspacePath);
  const isHttp = isHttpRequest(workspacePath, request.endpoint);
  const setRequestHeaders = useStore(s => s.setRequestHeaders);
  const declared = declaredContentType(request.headers);
  const inferred = contentTypeOf(request.bodies[0] ?? '');
  const bodyOnBodilessVerb = isHttp && bodyWithoutAMethodForIt(splitEndpoint(request.endpoint).method, request.bodies);
  const isForm = isHttp
    && request.bodies.length === 1
    && (declared ?? inferred).startsWith('application/x-www-form-urlencoded');
  const [fieldsOpen, setFieldsOpen] = useState(false);
  const [fieldRows, setFieldRows] = useState<QueryParam[]>([]);
  const fieldsRef = useDismiss<HTMLDivElement>(fieldsOpen, useCallback(() => setFieldsOpen(false), []));
  const writeFields = (rows: QueryParam[]) => {
    setFieldRows(rows);
    setRequestBody(0, joinForm(rows));
  };

  return (
    <div className={`body-editor${single ? ' is-single' : ''}`}>
      {isHttp ? (
      <div className="bar msg-head">
        <Layers size={12} />
        <span>{request.bodies.length === 0 ? 'no body' : 'body'}</span>
        {bodyOnBodilessVerb && (
          <span
            className="badge is-pending"
            title={`A ${splitEndpoint(request.endpoint).method || 'GET'} is sent with this body exactly as written — most servers ignore it, and some refuse the request`}
          >
            body on a {splitEndpoint(request.endpoint).method || 'GET'}
          </span>
        )}
        {request.bodies.length > 0 && (
          declared ? (
            <span className="mono muted content-type" title="Sent as this request's content-type — the REQUEST_HEADERS section says so">
              {declared}
            </span>
          ) : (
            <button
              className="mono muted content-type is-guess"
              onClick={() => setRequestHeaders({ ...request.headers, 'content-type': inferred })}
              title={`Nothing names a content-type, so ${inferred} is sent — click to write it into the request`}
            >
              {inferred} · inferred
            </button>
          )
        )}
        {isForm && (
          <div className="picker" ref={fieldsRef}>
            <button
              className={`btn is-ghost is-sm http-params${fieldRows.length > 0 ? ' is-on' : ''}`}
              onClick={() => { setFieldRows(splitForm(request.bodies[0] ?? '')); setFieldsOpen(v => !v); }}
              aria-haspopup="menu"
              aria-expanded={fieldsOpen}
              title="Edit this form as the fields it holds"
            >
              fields{fieldRows.length > 0 && <span className="badge">{fieldRows.length}</span>}
            </button>
            <Popover open={fieldsOpen} anchor={fieldsRef} align="end">
              <PairRows
                noun="field"
                rows={fieldRows}
                empty="No fields yet."
                onChange={writeFields}
              />
            </Popover>
          </div>
        )}
        <span className="grow" />
        {request.bodies.length === 0 ? (
          <button className="btn is-ghost is-sm" onClick={addRequestBody} title="Send a body with this request">
            <Plus size={12} /> add a body
          </button>
        ) : (
          <button
            className="btn is-ghost is-sm"
            onClick={() => removeRequestBody(request.bodies.length - 1)}
            title="Send this request without a body"
          >
            <X size={12} /> no body
          </button>
        )}
      </div>
      ) : (
      <div className="bar msg-head">
        <Layers size={12} />
        <span>{count(request.bodies.length, 'message')}</span>
        <span className={`bar${shapeMismatch ? ' shape-warn' : ' muted'}`}>
          <Send size={10} /> {shape}
          {wireNote && <span> — {wireNote}</span>}
        </span>
        <span className="grow" />
        <button
          className="btn is-ghost is-sm"
          onClick={addRequestBody}
          title={method && !method.clientStreaming
            ? 'This method takes one message — a second one is sent as its own call'
            : 'One more message, sent in this order'}
        >
          <Plus size={12} /> Add message
        </button>
      </div>
      )}

      {everyMessageSkipped(collectionParsed, request.bodies.length, isHttp ? 'httf' : 'gctf') && (
        <div className="note is-warn">
          Every message here carries <span className="mono">#[skip]</span>, and a run with nothing to
          send does not skip the call: it sends one empty <span className="mono">{'{}'}</span> in
          their place. The answer a run gets back is the answer to that, not to any body below.
        </div>
      )}

      {request.bodies.map((body, idx) => {
        const notJson = jsonProblem(body);
        const problem = isHttp ? null : notJson;
        const runs = messageRun(collectionParsed, idx, isHttp ? 'httf' : 'gctf');
        const written = bodyAsWritten(collectionParsed, idx, body);
        return (
        <div key={idx} className={`msg${problem ? ' is-bad' : ''}${runs.skipped ? ' is-skipped' : ''}`}>
          <div className="bar">
            <span className="msg-index mono">#{idx + 1}</span>
            {runs.skipped && (
              <span
                className="badge is-pending"
                title={isHttp
                  ? 'A run sends this call without a body — #[skip] on its REQUEST. Execute sends it anyway.'
                  : 'A run does not send this message — #[skip] on its REQUEST. Execute sends it anyway.'}
              >
                skipped
              </span>
            )}
            {written !== null && (
              <span
                className="muted mono body-as-written"
                title={`The file writes this message as:\n\n${written.text}\n\nEditing it here saves it as the JSON above.`}
              >
                {written.kind === 'json5'
                  ? 'written as JSON5'
                  : written.text.includes('\n') ? 'written differently' : 'written on one line'}
              </span>
            )}
            {runs.repeat !== null && (
              <span className="badge is-kind" title={`A run sends this message ${runs.repeat} times — #[repeat(${runs.repeat})] on its REQUEST. Execute sends it once.`}>
                ×{runs.repeat}
              </span>
            )}
            {isMulti && (
              <>
                <button
                  className="btn is-ghost is-icon"
                  onClick={() => moveRequestBody(idx, idx - 1)}
                  disabled={idx === 0}
                  title={idx === 0 ? 'This one goes first' : `Send message ${idx + 1} before ${idx}`}
                  aria-label={`Move message ${idx + 1} earlier`}
                >
                  <ChevronUp size={11} />
                </button>
                <button
                  className="btn is-ghost is-icon"
                  onClick={() => moveRequestBody(idx, idx + 1)}
                  disabled={idx === request.bodies.length - 1}
                  title={idx === request.bodies.length - 1 ? 'This one goes last' : `Send message ${idx + 1} after ${idx + 2}`}
                  aria-label={`Move message ${idx + 1} later`}
                >
                  <ChevronDown size={11} />
                </button>
                <button className="btn is-ghost is-icon" onClick={() => removeRequestBody(idx)} aria-label={`Remove message ${idx + 1}`}>
                  <X size={11} />
                </button>
              </>
            )}
            <button
              className="btn is-ghost is-icon"
              onClick={() => duplicateRequestBody(idx)}
              title="Copy this message, right after it"
              aria-label={`Duplicate message ${idx + 1}`}
            >
              <CopyPlus size={11} />
            </button>
            {problem && !isHttp && (
              <span className="msg-error" title={problem}>
                <TriangleAlert size={10} /> not JSON — {problem}
              </span>
            )}
            <span className="grow" />
            {fillErrors[idx] && <span className="msg-error" title={fillErrors[idx]}>{fillErrors[idx]}</span>}
            {body.trim() !== '' && <span className="badge">{humanBytes(byteSize(body))}</span>}
            <button
              className="btn is-ghost is-sm"
              onClick={() => {
                try { setRequestBody(idx, JSON.stringify(JSON.parse(body), null, 2)); } catch { /* invalid JSON formats to nothing */ }
              }}
              disabled={notJson !== null || body.trim() === ''}
              title={
                body.trim() === '' ? 'Nothing to indent yet'
                : notJson ? (isHttp ? 'Not JSON — nothing to indent' : 'Not JSON yet — nothing to indent')
                : 'Pretty-print this message'
              }
            >
              format
            </button>
            {!isHttp && (
            <button
              className="btn is-ghost is-sm"
              onClick={() => handleAutoFill(idx)}
              disabled={!request.endpoint || fillingIdxs.has(idx)}
              title={
                !request.endpoint ? 'Pick an endpoint first'
                : fillingIdxs.has(idx) ? 'Reading the schema…'
                : 'Fill this message from the proto schema'
              }
            >
              {fillingIdxs.has(idx) ? <Loader2 size={12} className="animate-spin" /> : <Sparkles size={12} />}
              Auto Fill
            </button>
            )}
          </div>

          <div
            className="editor"
            style={{ ['--body-lines' as string]: String(Math.max(1, body.split('\n').length)) } as CSSProperties}
          >
            <Editor
              height="100%"
              language={isHttp && jsonProblem(body) !== null ? 'plaintext' : 'json'}
              value={body}
              onChange={v => setRequestBody(idx, v || '')}
              theme={EDITOR_THEME}
              onMount={(ed, monaco) => {
                registerMonaco(monaco, themeMode);
                ensureJsonFormatter(monaco);
                registerEnvHoverProvider(monaco, currentEnv);
                repaintEnv.current = addEnvDecorations(ed, monaco, currentEnv, currentRuntime);
              }}
              options={{
                minimap: { enabled: false }, fontSize: 13,
                scrollBeyondLastLine: false, wordWrap: 'on',
                automaticLayout: true, lineNumbers: 'on', tabSize: 2,
                bracketPairColorization: { enabled: true },
              }}
            />
          </div>

          <EnvVarToolbar text={body} />
        </div>
        );
      })}
    </div>
  );
}
