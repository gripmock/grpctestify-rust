import { MonacoEditor as Editor } from '../MonacoEditor';
import { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import { problemMarkers } from '../../lib/problems';
import { bindingsOf, structuredSave, useStore, type SavePayloadSource } from '../../lib/store';
import { useShallow } from 'zustand/react/shallow';
import { extractLabel, extractValue, type ExtractValue } from '../../lib/extract-preview';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import { ensureGctfLanguage } from '../../lib/gctf-language';
import { draftFileName } from '../../lib/http-endpoint';
import { Save, Loader2, Check, Wand2, X, Pencil, TriangleAlert } from 'lucide-react';
import { isVariableName } from '../../lib/env';
import { splitExtractName, writtenExtractName } from '../../lib/extract-name';
import { sectionRun } from '../../lib/message-attributes';
import { unboundLines } from '../../lib/assert-problems';
import { sectionAsWritten } from '../../lib/body-as-written';
import { extractAudienceEmpty, extractionInput, previewSource, ranValue, reachLabel, reachOf, reachTitle } from '../../lib/extract-contract';
import { useModal } from 'luvo/ui/ModalContext';
import { useToast } from 'luvo/ui/ToastContext';
import { EDITOR_THEME, registerMonaco } from '../../lib/monaco-theme';
import { count } from 'luvo/data/plural';
import { answered } from '../../lib/response-seed';

const EMPTY_ASSERTS: string[] = [];
const EMPTY_TYPES: Record<string, string> = {};

const SEVERITY_MAP: Record<number, number> = { 1: 8, 2: 4, 3: 2, 4: 1 }; // LSP -> monaco.MarkerSeverity

function scrollableAncestor(from: HTMLElement | null): HTMLElement | null {
  for (let node = from?.parentElement ?? null; node; node = node.parentElement) {
    const overflow = getComputedStyle(node).overflowY;
    if ((overflow === 'auto' || overflow === 'scroll') && node.scrollHeight > node.clientHeight + 1) {
      return node;
    }
  }
  return null;
}

export function RawEditor() {
  const toast = useToast();
  const rawContent = useStore(s => s.rawContent);
  const loadRawContent = useStore(s => s.loadRawContent);
  const rawError = useStore(s => s.rawError);
  const setRawContent = useStore(s => s.setRawContent);
  const saveRawContent = useStore(s => s.saveRawContent);
  const diagnostics = useStore(s => s.diagnostics);
  const diagnosedText = useStore(s => s.diagnosedText);
  const themeMode = useStore(s => s.themeMode);
  const workspacePath = useStore(s => s.workspacePath);
  const rawOriginal = useStore(s => s.rawOriginal);

  const revealLine = useStore(s => s.revealLine);
  const clearReveal = useStore(s => s.clearReveal);
  const [saving, setSaving] = useState(false);
  const [formatting, setFormatting] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const monacoRef = useRef<any>(null);
  const editorRef = useRef<any>(null);

  useEffect(() => { loadRawContent(); }, [workspacePath, loadRawContent]);

  const saveFrom = useStore(useShallow((s): SavePayloadSource => ({
    collectionParsed: s.collectionParsed,
    protocol: s.protocol,
    request: s.request,
    address: s.address,
    addressTouched: s.addressTouched,
    protocolTouched: s.protocolTouched,
  })));
  const hasFile = useStore(s => s.workspacePath !== null);
  const draftName = useStore(s => draftFileName(s.workspacePath, s.request.endpoint));
  const payloadJson = useMemo(() => (hasFile ? '' : JSON.stringify(structuredSave(saveFrom))), [hasFile, saveFrom]);
  const preview = useDebouncedPost<{ content: string }>(
    '/api/preview-structured',
    payloadJson ? { ...JSON.parse(payloadJson), path: draftName } : null,
    400,
  );

  const applyReveal = useCallback((line: number) => {
    const editor = editorRef.current;
    if (!editor) return;
    editor.revealLineInCenter(line + 1);
    editor.setPosition({ lineNumber: line + 1, column: 1 });
    editor.focus();
    const dom = editor.getDomNode();
    const scroller = scrollableAncestor(dom);
    if (dom && scroller) {
      const lineY = dom.getBoundingClientRect().top + editor.getTopForLineNumber(line + 1);
      const box = scroller.getBoundingClientRect();
      scroller.scrollTop += lineY - (box.top + box.height / 2);
    }
    clearReveal();
  }, [clearReveal]);

  useEffect(() => {
    if (revealLine !== null) applyReveal(revealLine);
  }, [revealLine, applyReveal]);

  const paintMarkers = useCallback(() => {
    const monaco = monacoRef.current;
    const editor = editorRef.current;
    if (!monaco || !editor) return;
    const model = editor.getModel();
    if (!model) return;
    const markers = problemMarkers(diagnostics, diagnosedText, model.getValue()).map(m => ({
      startLineNumber: m.startLine,
      startColumn: m.startColumn,
      endLineNumber: m.endLine,
      endColumn: m.endColumn,
      message: m.message,
      severity: SEVERITY_MAP[m.severity] ?? monaco.MarkerSeverity.Error,
      code: m.code,
    }));
    monaco.editor.setModelMarkers(model, 'grpctestify', markers);
  }, [diagnostics, diagnosedText]);

  useEffect(() => { paintMarkers(); }, [paintMarkers]);

  const handleChange = (v: string | undefined) => setRawContent(v ?? '');

  const dirty = rawContent !== null && rawOriginal !== null && rawContent !== rawOriginal;

  const handleFormat = async () => {
    if (rawContent === null) return;
    setFormatting(true);
    setError(null);
    try {
      const changed = await useStore.getState().formatFile();
      toast.success(changed === 0 ? 'Already formatted' : `Formatted — ${count(changed, 'line')} changed`);
    } catch (err: any) {
      setError(err?.message || String(err));
    } finally {
      setFormatting(false);
    }
  };

  const handleSave = async () => {
    if (!workspacePath) { useStore.getState().requestSaveAs(); return; }
    setSaving(true);
    setError(null);
    try {
      await saveRawContent();
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (err: any) {
      setError(err?.message || String(err));
    } finally {
      setSaving(false);
    }
  };

  if (rawContent === null && !workspacePath) {
    const content = preview.data?.content ?? '';
    return (
      <div className="stack">
        <div className="bar">
          <span className="muted">The file this form would write. Save As… to edit it directly.</span>
        </div>
        <div className="editor">
          <Editor
            height="100%"
            language="gctf"
            value={content}
            theme={EDITOR_THEME}
            options={{ readOnly: true, minimap: { enabled: false }, fontSize: 12 }}
            onMount={(ed, monaco) => {
              requestAnimationFrame(() => ed.layout());
              registerMonaco(monaco, themeMode);
              ensureGctfLanguage(monaco);
            }}
          />
        </div>
      </div>
    );
  }

  if (rawContent === null) {
    if (rawError) {
      return (
        <div className="stack">
          <div className="assert is-fail"><span className="assert-mark">!</span><span>{rawError}</span></div>
          <div className="bar">
            <button className="btn is-sm" onClick={() => void loadRawContent()}>try again</button>
          </div>
        </div>
      );
    }
    return <div className="empty">Reading the file…</div>;
  }

  return (
    <div className="stack">
      <div className="bar">
        <span className="muted">
          {workspacePath
            ? 'Full file — edit ASSERTS/EXTRACT and any other section directly. Diagnostics match the LSP.'
            : 'This file has no name yet — Save writes it wherever you choose.'}
          {dirty && ' The tabs above catch up when this is saved.'}
        </span>
        <span className="grow" />
        {(dirty || !workspacePath) && <span className="badge is-pending">unsaved</span>}
        <button className="btn is-ghost is-sm" onClick={handleFormat} disabled={formatting} title="Format (same as grpctestify fmt)">
          {formatting ? <Loader2 size={12} className="animate-spin" /> : <Wand2 size={12} />}
          Format
        </button>
        <button className="btn is-ghost is-sm" onClick={handleSave} disabled={saving || (!dirty && !!workspacePath)}>
          {saving ? <Loader2 size={12} className="animate-spin" /> : saved ? <Check size={12} /> : <Save size={12} />}
          Save
        </button>
      </div>

      {error && (
        <div className="assert is-fail"><span className="assert-mark">!</span><span>{error}</span></div>
      )}

      <div className="editor">
        <Editor
          height="100%"
          language="gctf"
          value={rawContent}
          onChange={handleChange}
          theme={EDITOR_THEME}
          onMount={(ed, monaco) => {
            editorRef.current = ed;
            monacoRef.current = monaco;
            requestAnimationFrame(() => ed.layout());
            registerMonaco(monaco, themeMode);
            ensureGctfLanguage(monaco);
            paintMarkers();
            const pending = useStore.getState().revealLine;
            if (pending !== null) applyReveal(pending);
          }}
          options={{
            minimap: { enabled: false }, fontSize: 13,
            scrollBeyondLastLine: false, wordWrap: 'on',
            automaticLayout: true, lineNumbers: 'on', tabSize: 2,
          }}
        />
      </div>
    </div>
  );
}

export function ExtractsView({ extracts }: { extracts: Record<string, string> }) {
  const addExtract = useStore(s => s.addExtract);
  const removeExtract = useStore(s => s.removeExtract);
  const documents = useStore(s => s.documents);
  const activeStep = useStore(s => s.activeStep);
  const asserts = useStore(s => s.collectionParsed?.asserts) ?? EMPTY_ASSERTS;
  const response = useStore(s => s.response);
  const renameExtractVariable = useStore(s => s.renameExtractVariable);
  const types = useStore(s => s.collectionParsed?.extract_types) ?? EMPTY_TYPES;
  const skipped = useStore(s => sectionRun(s.collectionParsed, 'EXTRACT').skipped);
  const written = useStore(s => sectionAsWritten(s.collectionParsed, 'EXTRACT'));
  const diagnostics = useStore(s => s.diagnostics);
  const unbound = useMemo(() => unboundLines(diagnostics), [diagnostics]);
  const toast = useToast();
  const modal = useModal();

  const input = answered(response) ? extractionInput(response!.messages) : null;
  const ran = useStore(bindingsOf);
  const source = previewSource(response, activeStep, input?.total ?? 0, (ran?.length ?? 0) > 0);

  const [values, setValues] = useState<Record<string, ExtractValue>>({});
  const [checking, setChecking] = useState(false);

  const check = async (against: unknown, filters: Record<string, string>) => {
    setChecking(true);
    try {
      const pairs = await Promise.all(
        Object.entries(filters).map(async ([name, expr]) => {
          try {
            const res = await fetch('/api/eval/query', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ input: against, expr, runs: 0 }),
            });
            if (!res.ok) return [name, extractValue([], await res.text())] as const;
            const data = await res.json();
            return [name, extractValue(data.outputs, data.error)] as const;
          } catch (err: any) {
            void err;
            return [name, extractValue([], 'The workbench could not be reached — this filter was not run')] as const;
          }
        }),
      );
      setValues(Object.fromEntries(pairs));
    } finally {
      setChecking(false);
    }
  };
  const signature = JSON.stringify([input?.message ?? null, extracts]);
  const lastChecked = useRef<string | null>(null);
  useEffect(() => {
    if (!source.ok || Object.keys(extracts).length === 0) return;
    if (lastChecked.current === signature) return;
    lastChecked.current = signature;
    void check(input?.message, extracts);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, source.ok]);

  const [name, setName] = useState('');
  const [expr, setExpr] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState({ name: '', expr: '' });
  const [refused, setRefused] = useState<string | null>(null);
  const keys = Object.keys(extracts);

  const trimmed = name.trim();
  const typed = splitExtractName(trimmed);
  const nameProblem =
    trimmed === '' ? null
    : !isVariableName(typed.name) ? 'A name is a letter or _ followed by letters, digits, _ or . — and may end in :type'
    : keys.includes(typed.name) ? `${typed.name} is already extracted`
    : null;

  const submit = () => {
    if (!trimmed || !expr.trim() || nameProblem) return;
    addExtract(name, expr);
    setName('');
    setExpr('');
  };

  const commit = async (key: string) => {
    const written = draft.name.trim();
    const { name: nextName } = splitExtractName(written);
    const nextExpr = draft.expr.trim();
    const why =
      !nextName ? 'A name is what later steps type — an extraction cannot lose it'
      : !isVariableName(nextName) ? 'A name is a letter or _ followed by letters, digits, _ or . — and may end in :type'
      : !nextExpr ? 'A row needs a filter — remove the row instead'
      : nextName !== key && keys.includes(nextName) ? `${nextName} is already extracted`
      : null;
    if (why) { setRefused(why); return; }
    if (nextName !== key) {
      const reach = reachOf(key, documents, activeStep, asserts);
      const readElsewhere = reach.kind !== 'none';
      if (readElsewhere) {
        const outcome = await renameExtractVariable(key, nextName);
        if ('refused' in outcome) { setRefused(outcome.refused); return; }
        setEditing(null);
        setRefused(null);
        toast.info(outcome.rewritten <= 1
          ? `{{${key}}} renamed to {{${nextName}}}`
          : `{{${key}}} renamed to {{${nextName}}} — ${outcome.rewritten - 1} reader${outcome.rewritten === 2 ? '' : 's'} with it`);
        if (nextExpr !== extracts[key]) addExtract(written, nextExpr);
        return;
      }
    }
    setEditing(null);
    setRefused(null);
    const sameName = nextName === key && written === writtenExtractName(key, types[key]);
    if (sameName && nextExpr === extracts[key]) return;
    if (nextName !== key) removeExtract(key);
    addExtract(written, nextExpr);
  };

  const drop = async (key: string) => {
    const reach = reachOf(key, documents, activeStep, asserts);
    if (reach.kind === 'steps') {
      const where = reach.steps.map(i => `step ${i + 1}`).join(', ');
      const ok = await modal.confirm(
        'Remove extraction',
        `${where} reads {{${key}}}. Removing it leaves the placeholder unresolved.`,
        { confirmText: 'Remove', danger: true },
      );
      if (!ok) return;
    }
    removeExtract(key);
  };

  return (
    <div className="stack">
      {skipped && keys.length > 0 && (
        <div className="note is-warn">
          <span className="mono">#[skip]</span> on <span className="mono">EXTRACT</span> — a run walks
          past it, so nothing here is set and every later step reading {'{{name}}'} gets the braces.
        </div>
      )}
      {written !== null && keys.length > 0 && unbound.length === 0 && (
        <div className="note" title={written}>
          The file writes these with comments — editing a row here saves the section without them.
        </div>
      )}
      {unbound.length > 0 && (
        <div className="stack is-tight extract-unbound">
          {unbound.map((line, i) => (
            <div key={i} className="assert is-fail">
              <span className="assert-mark"><TriangleAlert size={12} /></span>
              <span className="stack is-tight">
                <span className="mono">{line}</span>
                <span className="assert-said">
                  This line binds nothing — a run reads past it. Write it as
                  <span className="mono"> name = filter</span>; saving from these rows writes the
                  bindings only, and this line would go.
                </span>
              </span>
            </div>
          ))}
        </div>
      )}
      {keys.length === 0 && unbound.length === 0 && (
        <EmptyMessage>No variable extractions — {extractAudienceEmpty(documents.length, activeStep)}</EmptyMessage>
      )}

      {keys.length > 0 && (
        <div className="bar">
          <span className="label grow">{source.note}</span>
          <button
            className="btn is-sm is-ghost"
            disabled={!source.ok || checking}
            onClick={() => void check(input?.message, extracts)}
            title={source.ok ? 'Run each filter again over the message the runner would read' : source.note}
          >
            {checking ? 'reading…' : 'check again'}
          </button>
        </div>
      )}
      {keys.length > 0 && (
        <div className="editor-frame stack extract-rows">
            {keys.map(k => {
              const reach = reachOf(k, documents, activeStep, asserts);
              return (
              <div key={k} className={`bar${editing === k ? ' extract-add' : ''}`}>
                {editing === k ? (
                  <>
                    <input
                      className="field field-frame mono"

                      value={draft.name}
                      autoFocus
                      spellCheck={false}
                      onChange={e => { setDraft(d => ({ ...d, name: e.target.value })); setRefused(null); }}
                      onKeyDown={e => {
                        if (e.key === 'Enter') { e.preventDefault(); void commit(k); }
                        if (e.key === 'Escape') { e.preventDefault(); setEditing(null); setRefused(null); }
                      }}
                    />
                    <input
                      className="field field-frame mono"
                      value={draft.expr}
                      spellCheck={false}
                      onChange={e => { setDraft(d => ({ ...d, expr: e.target.value })); setRefused(null); }}
                      onKeyDown={e => {
                        if (e.key === 'Enter') { e.preventDefault(); void commit(k); }
                        if (e.key === 'Escape') { e.preventDefault(); setEditing(null); setRefused(null); }
                      }}
                    />
                    <button className="btn is-sm is-ghost" onClick={() => void commit(k)}>done</button>
                  </>
                ) : (
                  <button
                    className="assert-expr grow"
                    onClick={() => {
                      setEditing(k);
                      setRefused(null);
                      setDraft({ name: writtenExtractName(k, types[k]), expr: extracts[k] });
                    }}
                    title="Edit this extraction"
                  >
                    <span className="extract-name">{'{{'}{k}{'}}'}</span>
                    {types[k] && (
                      <span className="badge is-info extract-kind" title={`Read as a ${types[k]} — \`$${k}\` in ASSERTS is one`}>
                        {types[k]}
                      </span>
                    )}
                    <span className="muted extract-eq">=</span>
                    <span className="mono grow">{extracts[k]}</span>
                    <Pencil size={10} className="assert-pencil" />
                  </button>
                )}
                <span
                  className={`badge ${reach.kind === 'none' ? 'is-pending' : 'is-info'} extract-reach`}
                  title={reachTitle(k, reach)}
                >
                  {reachLabel(reach)}
                </span>
                {values[k] ? (
                  <span className={`mono extract-value is-${values[k].kind}`} title={extractLabel(values[k])}>
                    {extractLabel(values[k])}
                  </span>
                ) : ranValue(ran, k) !== null && (
                  <span className="mono extract-value is-ok" title={`This file bound ${k} = ${ranValue(ran, k)}`}>
                    {ranValue(ran, k)}
                  </span>
                )}
                <button className="btn is-ghost is-icon" aria-label={`Remove ${k}`} onClick={() => void drop(k)}>
                  <X size={11} />
                </button>
              </div>
              );
            })}
        </div>
      )}
      {refused && <div className="assert-why"><span>{refused}</span></div>}
      <div className="bar extract-add">
        <input className={`field field-frame mono${nameProblem ? ' is-bad' : ''}`}  placeholder="name"
          value={name} onChange={e => setName(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') submit(); }} />
        <span className="muted">=</span>
        <input className="field field-frame mono" placeholder="path or filter"
          value={expr} onChange={e => setExpr(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') submit(); }} />
        <button
          className="btn is-ghost is-sm"
          onClick={submit}
          disabled={!trimmed || !expr.trim() || nameProblem !== null}
          title={
            nameProblem ?? (!trimmed ? 'Name the variable later documents will read'
            : !expr.trim() ? 'A jq filter over the response' : 'Add this to EXTRACT')
          }
        >
          Add
        </button>
      </div>
      {nameProblem && <div className="assert-why"><span>{nameProblem}</span></div>}
    </div>
  );
}

function EmptyMessage({ children }: { children: React.ReactNode }) {
  return <div className="empty">{children}</div>;
}
