import { useMemo, useState } from 'react';
import { addressSourceOf, callAddress, rawIsAuthoritative, structuredSave, useStore, workspaceDirty, type SavePayloadSource } from '../../lib/store';
import { useShallow } from 'zustand/react/shallow';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import { aboutWholeFile, countBySeverity, diagnosticsVoice, matchLineExact, sortProblems, severityLabel } from '../../lib/problems';
import { fixFor } from '../../lib/problem-fix';
import { previewRequest } from '../../lib/preview-request';
import { applyRange, applyRewrites, rewriteOf } from '../../lib/text-edit';
import { callFailed } from '../../lib/call-outcome';
import { draftFileName, isHttpRequest } from '../../lib/http-endpoint';
import { useToast } from 'luvo/ui/ToastContext';
import type { GctfDiagnostic } from '../../lib/types';
import { CircleAlert, TriangleAlert, Info, Check, ChevronRight, ChevronDown } from 'lucide-react';
import { useEffect } from 'react';
import { serverAnswered } from '../../lib/answer-source';

export function ProblemsRow() {
  const rawContent = useStore(s => s.rawContent);
  const rawOriginal = useStore(s => s.rawOriginal);
  const revealInRaw = useStore(s => s.revealInRaw);
  const workspacePath = useStore(s => s.workspacePath);
  const draftName = useStore(s => draftFileName(s.workspacePath, s.request.endpoint));
  const endpoint = useStore(s => s.request.endpoint);
  const activeStep = useStore(s => s.activeStep);
  const [open, setOpen] = useState(false);
  const toast = useToast();
  const loadRawContent = useStore(s => s.loadRawContent);
  useEffect(() => {
    if (workspacePath && rawContent === null) void loadRawContent();
  }, [workspacePath, rawContent, loadRawContent]);
  const fixCtx = useStore(useShallow(s => ({
    hasResponse: serverAnswered(s.response),
    failed: callFailed(s.response, isHttpRequest(s.workspacePath, s.request.endpoint)),
    http: isHttpRequest(s.workspacePath, s.request.endpoint),
    addressFromHeader: addressSourceOf(s) === 'typed' ? callAddress(s) : null,
  })));

  const saveFrom = useStore(useShallow((s): SavePayloadSource => ({
    collectionParsed: s.collectionParsed,
    protocol: s.protocol,
    request: s.request,
    address: s.address,
    addressTouched: s.addressTouched,
    protocolTouched: s.protocolTouched,
  })));
  const payloadJson = useMemo(() => JSON.stringify(structuredSave(saveFrom)), [saveFrom]);
  const payload = useMemo(() => JSON.parse(payloadJson), [payloadJson]);

  const parseError = useStore(s => s.parseError);
  const rawWins = rawIsAuthoritative({ rawContent, rawOriginal, parseError });

  const preview = useDebouncedPost<{ content: string }>(
    '/api/preview-structured',
    rawWins || !endpoint
      ? null
      : previewRequest(payload, {
          path: workspacePath ?? draftName,
          originalPath: workspacePath,
          activeStep,
        }),
    400,
  );

  const content = rawWins ? rawContent : preview.data?.content ?? null;
  const editable = rawContent !== null && content === rawContent;
  const source: 'raw' | 'form' = rawWins ? 'raw' : 'form';

  const voice = useStore(s => diagnosticsVoice({ path: s.workspacePath, dirty: workspaceDirty(s) }));

  const { data } = useDebouncedPost<GctfDiagnostic[]>(
    '/api/diagnostics',
    content === null
      ? null
      : { content, file_name: workspacePath ?? draftName, voice },
    250,
  );

  const problems = useMemo(() => {
    const found = sortProblems(data ?? []);
    if (!preview.error) return found;
    return [
      {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
        severity: 1,
        message: preview.error,
        source: 'play',
      } as GctfDiagnostic,
      ...found,
    ];
  }, [data, preview.error]);
  const counts = countBySeverity(problems);
  const rewrites = problems.filter(p => rewriteOf(p) !== null).length;
  const takeEveryRewrite = () => {
    if (rawContent === null) return;
    const { text, applied } = applyRewrites(rawContent, problems);
    if (applied === 0) {
      toast.error('Those lines have moved — nothing was rewritten');
      return;
    }
    useStore.getState().setRawContent(text);
    toast.success(`${applied} rewritten — save to keep ${applied === 1 ? 'it' : 'them'}`);
  };
  const nothing = content === null;
  const clean = !nothing && problems.length === 0;
  const publish = useStore(s => s.setProblemCount);
  const publishDiagnostics = useStore(s => s.setDiagnostics);

  useEffect(() => { publish(problems.length); }, [problems.length, publish]);
  useEffect(() => {
    if (content !== null) publishDiagnostics(problems, content);
  }, [problems, content, publishDiagnostics]);

  return (
    <div className="problems">
      <div className="bar problems-head">
      <button className="problems-toggle" onClick={() => setOpen(v => !v)} aria-expanded={open}>
        {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        <span className="label">Problems</span>
        {clean && <span className="badge is-ok" title="The same checks `grpctestify check` runs"><Check size={10} /> clean</span>}
        {nothing && <span className="muted">nothing to check yet</span>}
        {counts.errors > 0 && (
          <span className="badge is-fail" title={`${counts.errors} error${counts.errors > 1 ? 's' : ''} — the file would not check`}>
            <CircleAlert size={10} /> {counts.errors}
          </span>
        )}
        {counts.warnings > 0 && (
          <span className="badge is-pending" title={`${counts.warnings} warning${counts.warnings > 1 ? 's' : ''} — it checks, but something is off`}>
            <TriangleAlert size={10} /> {counts.warnings}
          </span>
        )}
        {counts.infos > 0 && (
          <span className="badge" title={`${counts.infos} note${counts.infos > 1 ? 's' : ''}`}>
            <Info size={10} /> {counts.infos}
          </span>
        )}
        {!open && problems.length > 0 && (
          <span className="problems-first">{problems[0].message}</span>
        )}
        <span className="grow" />
        {source === 'raw' && <span className="muted">checking the raw editor</span>}
      </button>
      {editable && rewrites > 1 && (
        <button
          className="btn is-sm problems-rewrite"
          title={`Take all ${rewrites} rewrites this file carries — the same edits \`grpctestify fmt -O\` would make`}
          onClick={takeEveryRewrite}
        >
          rewrite {rewrites}
        </button>
      )}
      </div>

      {open && problems.length > 0 && (
        <div className="problems-list">
          {problems.map((p, i) => {
            const kind = severityLabel(p.severity);
            const own = source === 'raw' || rawContent === null;
            const whole = aboutWholeFile(p);
            const found = own ? p.range.start.line : matchLineExact(content ?? '', p.range.start.line, rawContent);
            const inFile = !whole && found !== -1;
            const target = inFile ? found : p.range.start.line;
            return (
              <div key={`${p.code ?? ''}:${p.message}:${i}`} className={`problem is-${kind}`}>
                <button
                  className="problem-open"
                  onClick={() => { if (inFile) revealInRaw(target); }}
                  title={
                    whole ? 'About the file, not a line in it'
                    : own ? 'Open in the source tab'
                    : inFile ? 'Open in the source tab — matched by the line’s own text, because the forms are ahead of the file'
                    : 'This line only exists in what a save would write; the source tab still shows the file as saved'
                  }
                >
                  <span className="problem-mark">
                    {kind === 'error' ? <CircleAlert size={11} /> : kind === 'warning' ? <TriangleAlert size={11} /> : <Info size={11} />}
                  </span>
                  <span className="mono problem-loc">{whole ? 'file' : `${inFile ? '' : '~'}L${target + 1}`}</span>
                  <span className="problem-msg">{p.message}</span>
                  {p.code != null && <span className="badge">{String(p.code)}</span>}
                </button>
                {(() => {
                  const fix = fixFor(p, { ...fixCtx, editable });
                  if (!fix) return null;
                  return (
                    <button
                      className="btn is-sm problem-fix"
                      title={fix.title}
                      onClick={() => {
                        if (fix.id === 'apply-rewrite') {
                          const rewrite = rewriteOf(p);
                          const next = rewrite === null || rawContent === null
                            ? null
                            : applyRange(rawContent, p.range, rewrite);
                          if (next === null) {
                            toast.error('That line has moved — the rewrite was not applied');
                            return;
                          }
                          useStore.getState().setRawContent(next);
                          toast.success('Rewritten — save to keep it');
                          return;
                        }
                        if (fix.id === 'name-address') {
                          if (useStore.getState().nameAddressInFile()) {
                            toast.success('ADDRESS written into what a save would write — save to keep it');
                          }
                          return;
                        }
                        if (useStore.getState().expectFromResponse()) {
                          toast.success('Expectation written from the answer this tab got back');
                        }
                      }}
                    >
                      {fix.label}
                    </button>
                  );
                })()}
              </div>
            );
          })}
        </div>
      )}

      {open && clean && (
        <div className="problems-list">
          <div className="empty">No problems — this file passes the same checks `grpctestify check` runs.</div>
        </div>
      )}
    </div>
  );
}
