import { MonacoEditor as Editor } from '../MonacoEditor';
import { Seg } from 'luvo/ui/Seg';
import { useState, useCallback, useEffect, useMemo } from 'react';
import { addressSourceOf, useStore } from '../../lib/store';
import { Loader2, Check, X, Layers, Crosshair, Copy, MoreHorizontal, Radar, ListChecks, Gauge, Upload } from 'lucide-react';
import { bodyLanguage, durationLabel, grpcStatusLabel, httpStatusLabel, httpStatusTone, sentHeaders, shortPath, byteSize, humanBytes } from '../../lib/format';
import { isHttpRequest } from '../../lib/http-endpoint';
import { shapeOfRequest } from '../../lib/shape';
import { BenchResults } from '../request/BenchResults';
import { errorText } from '../../lib/grpc-error';
import { explainFailure } from '../../lib/failure';
import { servicesOf } from '../../lib/schema-miss';
import { metaEmptyNote, outcomeBadge } from '../../lib/outcome';
import { numbersRounded } from '../../lib/expect-model';
import { Tabs } from 'luvo/ui/Tabs';
import { JsonPick } from './JsonPick';
import { metaActions, statusAction } from '../../lib/pick-actions';
import { startHint, startSteps } from '../../lib/start-here';
import { arrivals, isNotable } from '../../lib/stream-timing';
import { callAddress } from '../../lib/store';
import { benchFailure, verdictResponse } from '../../lib/jobs';
import { moveRowFocus, stepIndex, treeStep } from '../../lib/tree-keys';
import { assertWhy, groupByStep, isBlock, stepHeading, takeApart } from '../../lib/assert-line';
import { binaryType, previewKind, wireBytes, type PreviewKind } from '../../lib/http-body';
import { readText, writeText } from 'luvo/data/storage';
import { lineDiff } from 'luvo/data/diff';
import { Diff } from 'luvo/ui/Diff';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import { useToast } from 'luvo/ui/ToastContext';
import { EDITOR_THEME, registerMonaco } from '../../lib/monaco-theme';
import { count } from 'luvo/data/plural';
import { NOTHING_TO_EXPECT, serverAnswered } from '../../lib/answer-source';

function eventClass(shape: string): string {
  return shape === 'client' ? 'is-up' : 'is-down';
}

function eventArrow(shape: string): string {
  return shape === 'client' ? '↑' : '↓';
}

function msgPreview(msg: unknown, maxLen = 60): string {
  const s = JSON.stringify(msg);
  if (!s || s === 'null') return '(null)';
  return s.length > maxLen ? s.slice(0, maxLen) + '…' : s;
}

const PREVIEW_LIMIT = 1_000_000;

export function ResponsePanel() {
  const [showMore, setShowMore] = useState(false);
  const [showHuge, setShowHuge] = useState(false);
  const moreRef = useDismiss<HTMLDivElement>(showMore, useCallback(() => setShowMore(false), []));
  const live = useStore(s => s.response);
  const fromRun = useStore(s => (
    s.workspacePath && s.run.kind !== 'bench' ? s.run.verdicts[s.workspacePath] : undefined
  ));
  const response = live ?? verdictResponse(fromRun);
  const setResponseTab = useStore(s => s.setResponseTab);
  const responseTab = useStore(s => s.responseTab);
  const benchRan = useStore(s => s.run.kind === 'bench'
    && (s.run.benchProgress !== null || s.run.benchReport !== null || benchFailure(s.run) !== null));
  const selectedMsg = useStore(s => s.responseMessage);
  const setSelectedMsg = useStore(s => s.setResponseMessage);
  const [picking, setPicking] = useState(false);
  useEffect(() => {
    if (!picking) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setPicking(false); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [picking]);
  const hasExpectation = useStore(s => {
    const p = s.collectionParsed;
    return !!p && ((p.expect_responses?.length ?? 0) > 0 || !!p.expect_error);
  });
  const endpoint = useStore(s => s.request.endpoint);
  const bodies = useStore(s => s.request.bodies);
  const reflectionMethods = useStore(s => s.reflectionMethods);
  const assertCount = useStore(s => s.collectionParsed?.asserts.length ?? 0);
  const workspacePath = useStore(s => s.workspacePath);
  const benchPaths = useStore(s => s.benchPaths);
  const benchIsThisFile = benchRan && (benchPaths.length === 0 || (!!workspacePath && benchPaths.includes(workspacePath)));
  const benchElsewhere = benchRan && !benchIsThisFile ? benchPaths : null;
  const benchRefused = useStore(s => benchFailure(s.run) !== null);

  const runJobId = useStore(s => s.runJobId);
  const runTest = useStore(s => s.runTest);
  const runData = useStore(s => s.runData);
  const revealInRaw = useStore(s => s.revealInRaw);
  const openJq = useStore(s => s.openJq);
  const addAssert = useStore(s => s.addAssert);
  const setRequestTab = useStore(s => s.setRequestTab);

  const themeMode = useStore(s => s.themeMode);
  const toast = useToast();

  const msgs: unknown[] = response?.messages ?? [];
  const msgCount = msgs.length;
  const manyCalls = msgCount > 1 && response?.shape === 'unary';
  const isStreaming = msgCount > 1 && !manyCalls;

  const documentsForAnswer = useStore(s => s.documents);
  const answerStep = response?.fromStep;
  const answeredBy = answerStep !== undefined ? documentsForAnswer[answerStep]?.endpoint : undefined;
  const isHttp = isHttpRequest(workspacePath, answeredBy ?? endpoint);
  const declaredBytes = isHttp && msgCount === 1 ? wireBytes(response?.headers ?? {}) : null;
  const payloadBytes = declaredBytes ?? (msgCount > 0 ? byteSize(msgs.length === 1 ? msgs[0] : msgs) : 0);
  const code = response?.statusCode ?? null;
  const documents = useStore(s => s.documents);
  const selectStep = useStore(s => s.selectStep);
  const activeStep = useStore(s => s.activeStep);
  const preview = msgCount === 1 ? previewKind(response?.headers ?? {}, msgs[0]) : null;
  const binary = isHttp && msgCount === 1 ? binaryType(response?.headers ?? {}) : null;
  const [showPreview, setShowPreview] = useState(() => readText('play.response.preview', 'on') !== 'off');
  useEffect(() => { writeText('play.response.preview', showPreview ? 'on' : 'off'); }, [showPreview]);
  const checkGroups = useMemo(
    () => groupByStep(response?.assertions ?? [], documents),
    [response?.assertions, documents],
  );
  const statusLabel = isHttp ? httpStatusLabel(code) : grpcStatusLabel(code);
  const landedElsewhere = response?.headers?.[':url'] ?? null;
  const rounded = (response?.messagesRaw ?? []).some(numbersRounded);
  const sentTo = useStore(s => s.lastCallAddress) ?? '';
  const shape = shapeOfRequest(endpoint, bodies.length, reflectionMethods, msgCount, response?.shape ?? null);
  const offsets = response?.messageOffsetsMs;
  const timeline = useMemo(() => arrivals(offsets ?? []), [offsets]);

  const copyText = async (text: string, what: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(`${what} copied`);
    } catch {
      toast.error('The browser refused the clipboard');
    }
  };

  const copyJson = async (value: unknown, what: string) => {
    const text = typeof value === 'string' ? value : JSON.stringify(value, null, 2);
    try {
      await navigator.clipboard.writeText(text);
      toast.success(`${what} — ${humanBytes(byteSize(text))} copied`);
    } catch {
      toast.error('The browser refused the clipboard');
    }
  };

  const uncheckedAsserts = response && !response.assertions && response.status === 'ok'
    ? assertCount
    : 0;

  const shownHeaders = useMemo(() => sentHeaders(response?.headers ?? {}), [response?.headers]);
  const pickable = msgs.length > 0 && typeof msgs[selectedMsg] === 'object' && msgs[selectedMsg] !== null;
  const metaCount = Object.keys(shownHeaders).length + Object.keys(response?.trailers ?? {}).length;

  const tabs = response?.assertions
    ? (['response', 'assertions', 'headers'] as const)
    : (['response', 'headers'] as const);

  return (
    <fieldset className="panel">
      <legend>response</legend>
      <div className="panel-body stack">

        <div className="bar answer-head">
          {benchIsThisFile && <span className="label">bench</span>}

          {response && (() => {
            const badge = outcomeBadge(response, isHttp);
            const tone = badge.kind === 'ok' ? ' is-ok'
              : badge.kind === 'pending' ? ' is-pending'
              : badge.kind === 'refused' ? ' is-warn'
              : ' is-fail';
            return (
              <span className={`badge${tone}`} title={badge.title}>
                {badge.kind === 'pending' ? <Loader2 size={11} className="animate-spin" />
                  : badge.kind === 'ok' ? <Check size={11} />
                  : <X size={11} />}
                {' '}{badge.label}
              </span>
            );
          })()}
          {response?.fromRun && (
            <span className="badge" title="Captured by the run that marked this file failed">
              from the run
            </span>
          )}
          {response?.fromCase && (
            <span className="badge is-info mono" title={`This is the answer of the case ${response.fromCase} — a file driven by rows runs once per row`}>
              {response.fromCase}
            </span>
          )}
          {documents.length > 1 && response?.fromStep !== undefined && (
            <button
              className="badge is-pick mono"
              onClick={() => selectStep(response.fromStep!)}
              title={`This is what step ${response.fromStep + 1} answered — show that step`}
            >
              step {response.fromStep + 1}
            </button>
          )}
          {preview && (
            <Seg
              className="preview-seg"
              label="How to show what came back"
              value={showPreview ? 'preview' : 'source'}
              onChange={v => setShowPreview(v === 'preview')}
              options={[
                { value: 'preview', label: 'preview', title: preview === 'svg' ? 'Draw the image' : 'Render the page — scripts do not run' },
                { value: 'source', label: 'source', title: 'Read the markup as it arrived' },
              ]}
            />
          )}
          {response && (response.durationMs != null || payloadBytes > 0) && (
            <span className="muted mono">
              {[response.durationMs != null ? durationLabel(response.durationMs) : null,
                payloadBytes > 0 ? humanBytes(payloadBytes) : null].filter(Boolean).join(' · ')}
            </span>
          )}
          {statusLabel && (isHttp || code !== 0) && (
            isHttp && code !== null
              ? (
                <button
                  className={`badge is-${httpStatusTone(code) ?? 'fail'} is-pick`}
                  title={`Assert this status — writes ${statusAction(code).line} into ASSERTS`}
                  onClick={() => {
                    if (!useStore.getState().focusAnswerStep()) {
                      toast.refuse(`This answer is step ${(useStore.getState().response?.fromStep ?? 0) + 1}'s — save or discard this step's edits first`);
                      return;
                    }
                    const said = addAssert(statusAction(code).line);
                    setRequestTab('asserts');
                    if (said === 'duplicate') toast.info('This file already asserts that');
                    else toast.success('Assertion added — Save writes it to the file');
                  }}
                >
                  {statusLabel} <ListChecks size={10} />
                </button>
              )
              : <span className={`badge is-fail`}>{statusLabel}</span>
          )}
          {uncheckedAsserts > 0 && (
            <button
              className="btn is-sm is-ghost"
              disabled={!workspacePath || runJobId !== null}
              onClick={() => workspacePath && void runTest()}
              title={[
                !workspacePath ? 'Save the file first — a run reads it from disk'
                : runJobId !== null ? 'A run is already going'
                : runData ? `Run the saved file once per row of ${runData}, so the ASSERTS are evaluated`
                : 'Run the saved file so the ASSERTS are evaluated the way CI evaluates them',
                'The EXPECT tab checks them against the answer on screen without calling again — this is the run\'s own verdict.',
              ].join('\n')}
            >
              {count(uncheckedAsserts, 'assert')} — not checked by a run
            </button>
          )}
          {landedElsewhere && (
            <span
              className="badge is-info mono"
              title={`The call was sent to ${sentTo} and followed to ${landedElsewhere} — redirects are followed, so the status above is the last hop's`}
            >
              followed to {landedElsewhere}
            </span>
          )}
          {!!isStreaming && (
            <span className="badge is-kind kind-down"><Layers size={10} /> streaming</span>
          )}
          {rounded && (
            <span
              className="badge is-pending"
              title="This answer carries a number wider than the panel can show exactly — what is written into the file is the text that came back"
            >
              shown rounded
            </span>
          )}
          {response?.messagesTruncated && (
            <span
              className="badge is-pending"
              title={`The call produced ${response.messagesTotal ?? msgCount} messages; the workbench keeps the first ones so the panel stays a panel. A run keeps 20 for its report.`}
            >
              first {msgCount} of {response.messagesTotal ?? msgCount}
            </span>
          )}
          {manyCalls && (
            <span className="badge" title="A unary method takes one message per call — these are separate calls">
              <Layers size={10} /> {msgCount} calls
            </span>
          )}

          <span className="bar answer-acts">

          {response && response.status !== 'pending' && response.sent !== false && !binary && !serverAnswered(response) && (
            <span className="muted">{NOTHING_TO_EXPECT}</span>
          )}
          {response && response.status !== 'pending' && response.sent !== false && !binary && serverAnswered(response) && (
            <button
              className="btn is-ghost is-sm"
              onClick={() => {
                const wrote = useStore.getState().expectFromResponse();
                if (wrote) {
                  toast.success(hasExpectation ? 'Expectation replaced with this answer' : 'Expectation written from this answer');
                  return;
                }
                toast.refuse(response.fromStep !== undefined && response.fromStep !== activeStep
                  ? `This answer is step ${response.fromStep + 1}'s — save or discard this step's edits first`
                  : 'There is nothing in this answer to expect');
              }}
              title={
                response.error
                  ? `Write an ERROR section for this failure${hasExpectation ? ' — it replaces the expectation this file has' : ''}`
                  : msgCount === 0
                    ? `Write a RESPONSE section that passes only when the call comes back with no messages${hasExpectation ? ' — it replaces the expectation this file has' : ''}`
                    : `Write a RESPONSE section from ${msgCount === 1 ? 'this message' : `these ${msgCount} messages`}${hasExpectation ? ' — it replaces the expectation this file has' : ''}`
              }
            >
              <ListChecks size={12} /> {hasExpectation ? 'Replace expectation' : 'Expect this'}
              {documents.length > 1 && response.fromStep !== undefined && response.fromStep !== activeStep && (
                <span className="muted mono"> in step {response.fromStep + 1}</span>
              )}
            </button>
          )}

          {picking && (
            <button className="btn is-ghost is-sm is-on" onClick={() => setPicking(false)}>
              <Crosshair size={12} /> Done picking <span className="muted">Esc</span>
            </button>
          )}
          {msgCount > 0 && (
            <div ref={moreRef} className="picker">
              <button
                className="btn is-ghost is-icon is-sm"
                onClick={() => setShowMore(v => !v)}
                title="What to do with this response"
                aria-haspopup="menu"
                aria-expanded={showMore}
              >
                <MoreHorizontal size={13} />
              </button>
              <Popover open={showMore} anchor={moreRef} align="end">
                <div className="menu">
                  {response?.status === 'ok' && (
                    <button
                      className="menu-item"
                      disabled={!pickable}
                      onClick={() => { setShowMore(false); setPicking(true); }}
                      title={pickable
                        ? 'Click a field in the response to turn it into an assertion or extraction'
                        : 'This answer is text, not fields — assert it with a jq expression instead'}
                    >
                      <Crosshair size={13} /> Pick fields to assert…
                    </button>
                  )}
                  <button
                    className="menu-item"
                    onClick={() => { setShowMore(false); useStore.getState().setDrawerOpen(true); }}
                    title={msgCount > 1
                      ? `Opens on message ${selectedMsg + 1} — the one selected here`
                      : 'Opens on this response'}
                  >
                    Try in jq / regex <span className="muted">⌘⇧J</span>
                  </button>
                  {msgCount > 1 && (
                    <button
                      className="menu-item"
                      onClick={() => { setShowMore(false); void copyJson(msgs[selectedMsg], `message ${selectedMsg + 1}`); }}
                    >
                      <Copy size={13} /> Copy message {selectedMsg + 1}
                    </button>
                  )}
                  <button
                    className="menu-item"
                    onClick={() => { setShowMore(false); void copyJson(msgs.length === 1 ? msgs[0] : msgs, msgCount > 1 ? `${msgCount} messages` : 'response'); }}
                  >
                    <Copy size={13} /> Copy {msgCount > 1 ? 'every message' : pickable ? 'response JSON' : 'the response body'}
                  </button>
                </div>
              </Popover>
            </div>
          )}
          </span>
        </div>

        {benchIsThisFile && <BenchResults />}
        {benchElsewhere && benchRefused && <BenchResults />}

        {benchElsewhere && (
          <button
            className="btn is-sm is-ghost bench-elsewhere"
            onClick={() => void useStore.getState().loadCollection(benchElsewhere[0], { pin: true })}
            title={`The measurement running here is of ${benchElsewhere.join(', ')} — open it to read the numbers`}
          >
            <Gauge size={11} /> bench of {benchElsewhere.length === 1
              ? shortPath(benchElsewhere[0], 28)
              : `${benchElsewhere.length} files`}
          </button>
        )}

        {!response && !benchIsThisFile && !benchElsewhere && <StartHere />}

        {response && response.status !== 'pending' && (
          <>
            <Tabs
              label="What of the answer to show"
              items={tabs.map(tab => ({
                key: tab,
                label: (
                  <>
                    {tab === 'response' ? `response${msgCount > 1 ? ` (${msgCount})` : ''}`
                      : tab === 'assertions' ? 'checks'
                      : isHttp ? 'headers' : 'metadata'}
                    {tab === 'headers' && metaCount > 0 && <span className="badge">{metaCount}</span>}
                    {tab === 'assertions' && (
                      <span className={`badge${response.assertions!.every(a => a.passed) ? ' is-ok' : ' is-fail'}`}>
                        {response.assertions!.filter(a => a.passed).length}/{response.assertions!.length}
                      </span>
                    )}
                  </>
                ),
              }))}
              value={responseTab}
              onChange={setResponseTab}
            />

            {responseTab === 'response' && (
              <div className="stack">
                {msgCount === 0 && (
                  response.error
                    ? <FailureCard error={response.error} statusCode={response.statusCode ?? null} />
                    : response.fromRun
                      ? <div className="empty">The run kept the checks, not the body — it keeps bodies for failures. Execute to see this one.</div>
                      : <div className="empty">{isHttp ? 'The answer carried no body' : 'No response messages'}</div>
                )}

                {msgCount > 1 && (
                  <div className="events">
                    <div>
                      {msgs.map((msg, i) => (
                        <div
                          key={i}
                          role="button"
                          tabIndex={selectedMsg === i ? 0 : -1}
                          aria-pressed={selectedMsg === i}
                          className={`event ${eventClass(shape)}${selectedMsg === i ? ' is-on' : ''}`}
                          onClick={() => setSelectedMsg(i)}
                          onKeyDown={e => {
                            if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setSelectedMsg(i); }
                            const step = treeStep(e.key);
                            if (step === null) return;
                            e.preventDefault();
                            const at = stepIndex(i, msgCount, step);
                            setSelectedMsg(at);
                            moveRowFocus(e.currentTarget as HTMLElement, step, '.events .event');
                          }}
                          title={[
                            `#${i + 1} · ${humanBytes(byteSize(msg))}`,
                            timeline[i] ? `${durationLabel(timeline[i].at)} after the call started` : null,
                            timeline[i]?.gap != null ? `${durationLabel(timeline[i].gap)} after the message before it` : null,
                          ].filter(Boolean).join('\n')}
                        >
                          <span className="arrow">{eventArrow(shape)}</span>
                          <span className="body">{msgPreview(msg)}</span>
                          <span className="at">
                            {isNotable(timeline[i]?.gap ?? null) && (
                              <span className="gap">+{durationLabel(timeline[i].gap!)}</span>
                            )}
                            {timeline[i] != null ? durationLabel(timeline[i].at) : `#${i + 1}`}
                          </span>
                        </div>
                      ))}
                      {response.error && (
                        <div className="event is-err">
                          <span className="arrow">⚠</span>
                          <span className="body">{errorText(response.error)}</span>
                          <span className="at">{statusLabel ?? ''}</span>
                        </div>
                      )}
                      {!response.error && response.status === 'ok' && (
                        <div className="event is-end">
                          <span className="arrow">✓</span>
                          <span className="body">{manyCalls ? 'every call answered' : 'stream closed'}</span>
                          <span className="at">{response.durationMs != null ? durationLabel(response.durationMs) : ''}</span>
                        </div>
                      )}
                    </div>

                    <MessageView
                      value={msgs[selectedMsg] ?? {}}
                      messages={msgs}
                      picking={picking}
                      theme={EDITOR_THEME}
                      mode={themeMode}
                      shown={showHuge}
                      onShow={() => setShowHuge(true)}
                      headers={response.headers}
                    />
                  </div>
                )}

                {msgCount === 1 && binary && (
                  <div className="empty binary-body">
                    <span className="mono">{binary}</span>
                    <span>{payloadBytes} bytes — a binary answer is not shown, and not something to expect field by field.</span>
                    <span className="muted">Check it with <span className="mono">@status()</span> and a header, or with <span className="mono">@len()</span>.</span>
                  </div>
                )}
                {msgCount === 1 && !binary && (preview && showPreview
                  ? <PreviewFrame kind={preview} body={msgs[0] as string} />
                  : (
                    <MessageView
                      headers={response.headers}
                      value={msgs[0] ?? {}}
                      messages={msgs}
                      picking={picking}
                      theme={EDITOR_THEME}
                      mode={themeMode}
                      shown={showHuge}
                      onShow={() => setShowHuge(true)}
                    />
                  ))}
              </div>
            )}

            {responseTab === 'assertions' && response.assertions && (
              <div>
                {response.assertions.length === 0 && (
                  <div className="empty">
                    Nothing to check — this file has no {isHttp ? 'RESPONSE or ASSERTS' : 'RESPONSE, ERROR or ASSERTS'} section
                  </div>
                )}
                {checkGroups.map((group, g) => (
                  <div key={g} className="check-group">
                    {group.step && (
                      <button
                        className="check-step"
                        onClick={() => selectStep(group.step!.index)}
                        title={`Show step ${group.step.index + 1} in the request panel`}
                      >
                        <span className="mono">step {group.step.index + 1}</span>
                        <span className="muted mono grow">{stepHeading(group.step.endpoint ?? '', group.checks)}</span>
                      </button>
                    )}
                    {group.checks.map((a, i) => {
                  const why = a.passed ? null : assertWhy(a);
                  return (
                    <div key={i} className={`assert ${a.passed ? 'is-ok' : 'is-fail'}`}>
                      <span className="assert-mark">{a.passed ? <Check size={12} /> : <X size={12} />}</span>
                      <span className="stack is-cell">
                        <span className="bar">
                          <button
                            className="assert-at"
                            disabled={!workspacePath}
                            onClick={() => revealInRaw(a.line - 1)}
                            title={workspacePath
                              ? `Open line ${a.line} in the source tab`
                              : 'Save this as a file first — there is no source to open'}
                          >
                            L{a.line}
                          </button>
                          <span className="grow">{a.expression}</span>
                          {!a.passed && takeApart(a.expression) && (
                            <button
                              className="btn is-ghost is-sm"
                              onClick={() => openJq(a.expression)}
                              title="Open this filter in the jq tool, over this answer"
                            >
                              take apart
                            </button>
                          )}
                          {a.elapsed_ms > 0 && <span className="muted">{a.elapsed_ms} ms</span>}
                        </span>
                        {why && isBlock(why) && (
                          <span className="stack assert-diff">
                            {why.message && <span className="assert-said">{why.message}</span>}
                            <Diff lines={lineDiff(why.expected ?? '', why.actual ?? '')} />
                          </span>
                        )}
                        {why && !isBlock(why) && (
                          <span className="assert-why">
                            {why.message && <span className="assert-said">{why.message}</span>}
                            {why.expected !== null && (
                              <><span className="assert-key">expected</span><span className="mono">{why.expected}</span></>
                            )}
                            {why.actual !== null && (
                              <><span className="assert-key">actual</span><span className="mono">{why.actual}</span></>
                            )}
                          </span>
                        )}
                        {why?.hint && <span className="assert-remedy">{why.hint}</span>}
                      </span>
                    </div>
                  );
                    })}
                  </div>
                ))}
              </div>
            )}

            {responseTab === 'headers' && (
              <div className="editor-frame stack">
                {metaCount === 0 && (
                  <div className="muted">{metaEmptyNote(isHttp, !!response.fromRun)}</div>
                )}

                {Object.keys(shownHeaders).length > 0 && (
                  <MetaList
                    label="headers"
                    kind="headers"
                    rows={shownHeaders}
                    onCopy={(k, v) => void copyText(v, k)}
                  />
                )}

                {Object.keys(response.trailers || {}).length > 0 && (
                  <MetaList
                    label="trailers"
                    kind="trailers"
                    rows={response.trailers || {}}
                    onCopy={(k, v) => void copyText(v, k)}
                  />
                )}
              </div>
            )}

          </>
        )}
      </div>
    </fieldset>
  );
}

function MessageView({ value, picking, theme, mode, shown, onShow, messages, headers }: {
  value: unknown;
  picking: boolean;
  theme: string;
  mode: 'light' | 'dark';
  shown: boolean;
  onShow: () => void;
  headers: Record<string, string>;
  messages: unknown[];
}) {
  const asText = typeof value === 'string';
  const text = asText ? (value as string) : JSON.stringify(value, null, 2);

  if (text.length > PREVIEW_LIMIT && !shown) {
    return (
      <div className="editor huge-payload">
        <span className="muted">
          message preview larger than {humanBytes(PREVIEW_LIMIT)} is hidden
        </span>
        <button className="btn is-sm" onClick={onShow}>show anyway</button>
      </div>
    );
  }

  if (picking) {
    return <div className="editor"><JsonPick value={value} messages={messages} /></div>;
  }

  return (
    <div className="editor is-answer">
      <Editor
        height="100%"
        language={bodyLanguage(headers, !asText)}
        value={text}
        theme={theme}
        onMount={(_ed, monaco) => registerMonaco(monaco, theme === EDITOR_THEME ? mode : 'light')}
        options={{
          readOnly: true, minimap: { enabled: false }, fontSize: 13,
          scrollBeyondLastLine: false, wordWrap: 'on', automaticLayout: true,
          lineNumbers: 'on', folding: true,
          renderLineHighlight: 'none',
          domReadOnly: true,
          contextmenu: false,
        }}
      />
    </div>
  );
}

function metaValue(value: string) {
  return value === '' ? <span className="muted">(empty)</span> : value;
}

function MetaList({ label, kind, rows, onCopy }: {
  label: string;
  kind: 'headers' | 'trailers';
  rows: Record<string, string>;
  onCopy: (key: string, value: string) => void;
}) {
  const addAssert = useStore(s => s.addAssert);
  const setRequestTab = useStore(s => s.setRequestTab);
  const toast = useToast();
  const [open, setOpen] = useState<string | null>(null);
  const menuRef = useDismiss<HTMLDivElement>(open !== null, useCallback(() => setOpen(null), []));

  return (
    <div>
      <div className="label">{label}</div>
      <dl className="kv">
        {Object.entries(rows).map(([k, v]) => (
          <div key={k} className="bar meta-row">
            <dt>{k}</dt>
            <dd className="mono">
              <button
                className="meta-value"
                onClick={() => onCopy(k, v)}
                disabled={v === ''}
                title={v === '' ? 'Nothing to copy' : `Copy ${k}`}
              >
                {metaValue(v)}
              </button>
            </dd>
            <div className="picker meta-pick" ref={open === k ? menuRef : undefined}>
              <button
                className="btn is-ghost is-icon is-sm"
                onClick={() => setOpen(o => (o === k ? null : k))}
                aria-haspopup="menu"
                aria-expanded={open === k}
                aria-label={`Assert ${k}`}
                title={`Assert this ${kind === 'headers' ? 'header' : 'trailer'}`}
              >
                <ListChecks size={12} />
              </button>
              <Popover open={open === k} anchor={menuRef} align="end">
                <div className="menu" role="menu">
                  <div className="menu-group mono">{k}</div>
                  {metaActions(kind, k, v).map(action => (
                    <button
                      key={action.line}
                      className="menu-item mono"
                      onClick={() => {
                        if (!useStore.getState().focusAnswerStep()) {
                          const from = useStore.getState().response?.fromStep;
                          toast.refuse(from === undefined
                            ? 'This step has edits — save or discard them first'
                            : `This answer is step ${from + 1}'s — save or discard this step's edits first`);
                          setOpen(null);
                          return;
                        }
                        const said = addAssert(action.line);
                        setRequestTab('asserts');
                        setOpen(null);
                        if (said === 'duplicate') toast.info('This file already asserts that');
                        else toast.success('Assertion added — Save writes it to the file');
                      }}
                    >
                      {action.label}
                    </button>
                  ))}
                </div>
              </Popover>
            </div>
          </div>
        ))}
      </dl>
    </div>
  );
}

function StartHere() {
  const endpoint = useStore(s => s.request.endpoint);
  const isHttp = useStore(s => isHttpRequest(s.workspacePath, s.request.endpoint));
  const methodCount = useStore(s => s.reflectionMethods.length);
  const fileCount = useStore(s => s.visibleFiles.length);
  const hasWorkspaceFile = useStore(s => s.workspacePath !== null);
  const address = useStore(callAddress);
  const reflectionRefused = useStore(s => s.reflectStatus === 'error');
  const { title, hint, ready, ask, bring } = startHint({
    endpoint, methodCount, fileCount, hasWorkspaceFile, address, reflectionRefused,
  });
  const requestPick = useStore(s => s.requestPick);
  const firstRun = useStore(s => s.totalOk + s.totalError === 0);
  const reachable = useStore(s => (s.lastCallAddress === null ? null : s.response?.status !== 'error'));
  const aimedBy = useStore(addressSourceOf);
  const steps = startSteps({ endpoint, methodCount, address, reachable, defaulted: aimedBy === 'default' });

  if (firstRun) {
    return (
      <div className="empty stack start-steps">
        <span className="label">three steps to a first call</span>
        <ol className="stack">
          {steps.map((step, i) => (
            <li key={step.key} className={`start-step${step.done ? ' is-done' : ''}`}>
              <span className="start-mark">{step.done ? '✓' : i + 1}</span>
              <span>{step.label}</span>
              <span className="muted start-detail mono">{step.detail}</span>
              {step.key === 'method' && !step.done && ask && (
                <button className="btn is-sm start-action" onClick={requestPick}>
                  <Radar size={12} /> ask {address} what it serves
                </button>
              )}
              {step.key === 'send' && ready && (
                <span className="bar start-action">
                  <kbd className="kbd">⌘</kbd><kbd className="kbd">⏎</kbd>
                </span>
              )}
            </li>
          ))}
        </ol>
        <span className="muted start-hint">
          A call you like becomes a test: <span className="mono">save</span> writes{' '}
          {endpoint.trim() === ''
            ? <>a <span className="mono">.gctf</span> or <span className="mono">.httf</span> file — a service
              and a method makes the first, a method and a path the second</>
            : <>a <span className="mono">{isHttp ? '.httf' : '.gctf'}</span> file</>}
          , and the same file runs in CI.
        </span>
      </div>
    );
  }

  return (
    <div className="empty stack is-tight is-centred">
      <span>{title}</span>
      {ready ? (
        <span className="stack is-tight is-centred">
          <span className="bar">
            <kbd className="kbd">⌘</kbd>
            <kbd className="kbd">⏎</kbd>
            <span className="muted">to execute</span>
          </span>
          {hasWorkspaceFile && (
            <span className="bar">
              <kbd className="kbd">⌘</kbd>
              <kbd className="kbd">⇧</kbd>
              <kbd className="kbd">⏎</kbd>
              <span className="muted">to run the file — ASSERTS included</span>
            </span>
          )}
        </span>
      ) : (
        <span className="muted start-hint">{hint}</span>
      )}
      {(ask || bring) && (
        <span className="bar">
          {ask && (
            <button className="btn is-sm" onClick={requestPick}>
              <Radar size={12} /> ask {address} what it serves
            </button>
          )}
          {bring && (
            <button className="btn is-sm is-ghost" onClick={() => useStore.getState().requestImport()}>
              <Upload size={12} /> import a curl or grpcurl command
            </button>
          )}
        </span>
      )}
    </div>
  );
}

function FailureCard({ error, statusCode }: { error: string; statusCode: number | null }) {
  const dialled = useStore(callAddress);
  const reflected = useStore(s => s.reflectionMethods);
  const serves = useMemo(() => servicesOf(reflected), [reflected]);
  const failure = explainFailure(error, statusCode, dialled, serves);
  return (
    <div className="failure">
      <div className="assert is-fail">
        <span className="assert-mark">✗</span>
        <span>{failure.title}</span>
      </div>
      {failure.detail && <div className="mono failure-detail">{failure.detail}</div>}
      {failure.fixes.length > 0 && (
        <ul className="failure-fixes">
          {failure.fixes.map(fix => <li key={fix}>{fix}</li>)}
        </ul>
      )}
    </div>
  );
}

function PreviewFrame({ kind, body }: { kind: PreviewKind; body: string }) {
  return (
    <iframe
      className="preview-frame"
      sandbox=""
      title={kind === 'svg' ? 'The image the answer carried' : 'The page the answer carried'}
      srcDoc={body}
    />
  );
}
