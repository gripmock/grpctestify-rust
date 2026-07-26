import { MonacoEditor as Editor } from '../MonacoEditor';
import { useState, useRef, useEffect, useCallback } from 'react';
import get from 'lodash-es/get';
import { useStore } from '../../lib/store';
import { Tag, User, FileText, Shield, Wrench, Save, Loader2, Check } from 'lucide-react';
import { btn, colors } from '../../lib/theme';
import type { CollectionParsed } from '../../lib/types';
import { ProtoManager } from './ProtoManager';

export function GctfInfoPanel() {
  const p = useStore(s => s.collectionParsed);
  const tab = useStore(s => s.gctfTab);
  const setTab = useStore(s => s.setGctfTab);

  if (!p) return null;

  const tabs: { key: typeof tab; label: string; count?: number }[] = [
    { key: 'request', label: 'Config' },
    { key: 'raw', label: 'Raw' },
    { key: 'asserts', label: 'Asserts', count: get(p, 'asserts.length', 0) },
    { key: 'extracts', label: 'Extracts', count: Object.keys(get(p, 'extracts', {})).length },
    { key: 'meta', label: 'Info' },
    { key: 'proto', label: 'Proto' },
  ];

  return (
    <section style={{ marginTop: 8, borderTop: '1px solid var(--border)', paddingTop: 8 }}>
      <div style={{ display: 'flex', borderBottom: '1px solid var(--border)', marginBottom: 8 }}>
        {tabs.map(t => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            style={{
              display: 'flex', alignItems: 'center', gap: 4,
              padding: '5px 10px', fontSize: 11, cursor: 'pointer', border: 'none', background: 'none',
              color: tab === t.key ? 'var(--accent)' : 'var(--text-secondary)',
              fontWeight: tab === t.key ? 600 : 400,
              borderBottom: tab === t.key ? '2px solid var(--accent)' : '2px solid transparent',
            }}
          >
            {t.label}
            {t.count !== undefined && t.count > 0 && (
              <span style={{ fontSize: 10, background: 'var(--bg-tertiary)', borderRadius: 8, padding: '0 5px', color: 'var(--text-muted)' }}>
                {t.count}
              </span>
            )}
          </button>
        ))}
      </div>

      {tab === 'request' && <RequestInfo p={p} />}
      {tab === 'raw' && <RawEditor />}
      {tab === 'asserts' && <AssertsView asserts={p.asserts || []} />}
      {tab === 'extracts' && <ExtractsView extracts={p.extracts || {}} />}
      {tab === 'meta' && <MetaView p={p} />}
      {tab === 'proto' && <ProtoManager />}
    </section>
  );
}

const SEVERITY_MAP: Record<number, number> = { 1: 8, 2: 4, 3: 2, 4: 1 }; // LSP -> monaco.MarkerSeverity

function RawEditor() {
  const rawContent = useStore(s => s.rawContent);
  const loadRawContent = useStore(s => s.loadRawContent);
  const setRawContent = useStore(s => s.setRawContent);
  const saveRawContent = useStore(s => s.saveRawContent);
  const fetchDiagnostics = useStore(s => s.fetchDiagnostics);
  const monacoTheme = useStore(s => s.theme === 'dark' ? 'vs-dark' : 'light');
  const workspacePath = useStore(s => s.workspacePath);
  const rawOriginal = useStore(s => s.rawOriginal);

  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const monacoRef = useRef<any>(null);
  const editorRef = useRef<any>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => { loadRawContent(); }, [workspacePath]);

  const runDiagnostics = useCallback(async (value: string) => {
    const diags = await fetchDiagnostics(value);
    const monaco = monacoRef.current;
    const editor = editorRef.current;
    if (!monaco || !editor) return;
    const model = editor.getModel();
    if (!model) return;
    const markers = diags.map((d: any) => ({
      startLineNumber: d.range.start.line + 1,
      startColumn: d.range.start.character + 1,
      endLineNumber: d.range.end.line + 1,
      endColumn: d.range.end.character + 1,
      message: d.message,
      severity: SEVERITY_MAP[d.severity ?? 1] ?? monaco.MarkerSeverity.Error,
      code: typeof d.code === 'string' || typeof d.code === 'number' ? String(d.code) : undefined,
    }));
    monaco.editor.setModelMarkers(model, 'grpctestify', markers);
  }, [fetchDiagnostics]);

  const handleChange = (v: string | undefined) => {
    const value = v ?? '';
    setRawContent(value);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => runDiagnostics(value), 400);
  };

  useEffect(() => {
    if (rawContent !== null) runDiagnostics(rawContent);
  }, [workspacePath]);

  const dirty = rawContent !== null && rawOriginal !== null && rawContent !== rawOriginal;

  const handleSave = async () => {
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

  if (rawContent === null) {
    return <div style={{ padding: 12, fontSize: 12, color: 'var(--text-muted)' }}>Loading…</div>;
  }

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
        <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
          Full file — edit ASSERTS/EXTRACT and any other section directly. Diagnostics match the LSP.
        </span>
        <span style={{ flex: 1 }} />
        {dirty && <span style={{ fontSize: 10, color: colors.warning }}>unsaved</span>}
        <button onClick={handleSave} disabled={saving || !dirty} style={{ ...btn('ghost', 'sm'), opacity: dirty ? 1 : 0.4 }}>
          {saving ? <Loader2 size={12} className="animate-spin" /> : saved ? <Check size={12} color={colors.success} /> : <Save size={12} />}
          Save
        </button>
      </div>
      {error && <div style={{ fontSize: 11, color: colors.error, marginBottom: 6 }}>{error}</div>}
      <div style={{ border: '1px solid var(--border)', borderRadius: 6, overflow: 'hidden' }}>
        <Editor
          height="360px"
          language="plaintext"
          value={rawContent}
          onChange={handleChange}
          theme={monacoTheme}
          onMount={(ed, monaco) => {
            editorRef.current = ed;
            monacoRef.current = monaco;
            runDiagnostics(rawContent);
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

function RequestInfo({ p }: { p: CollectionParsed }) {
  const rows: { label: string; value: string; icon: React.ReactNode }[] = [];
  if (p.address) rows.push({ label: 'ADDRESS', value: p.address, icon: <Wrench size={12} /> });
  if (p.endpoint) rows.push({ label: 'ENDPOINT', value: p.endpoint, icon: <FileText size={12} /> });
  if (Object.keys(get(p, 'tls', {})).length) rows.push({ label: 'TLS', value: JSON.stringify(p.tls, null, 2), icon: <Shield size={12} /> });
  if (Object.keys(get(p, 'options', {})).length) rows.push({ label: 'OPTIONS', value: JSON.stringify(p.options, null, 2), icon: <Wrench size={12} /> });
  if (Object.keys(get(p, 'proto', {})).length) {
    const protoStr = Object.entries(p.proto)
      .map(([k, v]) => `${k}: ${v}`).join('\n');
    rows.push({ label: 'PROTO', value: protoStr, icon: <FileText size={12} /> });
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      {rows.map(r => (
        <div key={r.label} style={{ border: '1px solid var(--border)', borderRadius: 6, overflow: 'hidden' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '3px 8px', fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', background: 'var(--bg-tertiary)', textTransform: 'uppercase' }}>
            {r.icon} {r.label}
          </div>
          <div style={{ padding: '4px 8px', fontSize: 12, fontFamily: 'monospace', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
            {r.value}
          </div>
        </div>
      ))}
      {rows.length === 0 && <EmptyMessage>No additional configuration in this file</EmptyMessage>}
    </div>
  );
}

function AssertsView({ asserts }: { asserts: string[] }) {
  const monoTheme = useStore(s => s.theme === 'dark' ? 'vs-dark' : 'light');
  if (asserts.length === 0) return <EmptyMessage>No assertions</EmptyMessage>;
  return (
    <div style={{ border: '1px solid var(--border)', borderRadius: 6, overflow: 'hidden' }}>
      <Editor
        height={Math.min(200, (asserts.length || 0) * 22)}
        language="plaintext"
        value={asserts.map((a, i) => `${i + 1}. ${a}`).join('\n')}
        theme={monoTheme}
        options={{ readOnly: true, minimap: { enabled: false }, fontSize: 12, lineNumbers: 'off', scrollBeyondLastLine: false }}
      />
    </div>
  );
}

function ExtractsView({ extracts }: { extracts: Record<string, string> }) {
  const keys = Object.keys(extracts);
  if (keys.length === 0) return <EmptyMessage>No variable extractions</EmptyMessage>;
  return (
    <div style={{ border: '1px solid var(--border)', borderRadius: 6, fontSize: 12, fontFamily: 'monospace' }}>
      {keys.map(k => (
        <div key={k} style={{ padding: '4px 8px', borderBottom: '1px solid var(--border)', display: 'flex', gap: 6 }}>
          <span style={{ color: 'var(--accent)' }}>{`\$\{${k}\}`}</span>
          <span style={{ color: 'var(--text-muted)' }}>=</span>
          <span>{extracts[k]}</span>
        </div>
      ))}
    </div>
  );
}

function MetaView({ p }: { p: CollectionParsed }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 12 }}>
      {p.meta_name && <MetaRow icon={<User size={12} />} label="Name" value={p.meta_name} />}
      {p.meta_owner && <MetaRow icon={<User size={12} />} label="Owner" value={p.meta_owner} />}
      {p.meta_summary && <MetaRow icon={<FileText size={12} />} label="Summary" value={p.meta_summary} />}
            {get(p, 'meta_tags.length', 0) > 0 && (
        <MetaRow
          icon={<Tag size={12} />}
          label="Tags"
          value={
            <span style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
              {p.meta_tags.map(t => (
                <span key={t} style={{ fontSize: 10, background: 'var(--bg-tertiary)', borderRadius: 8, padding: '0 6px', color: 'var(--text-muted)' }}>
                  {t}
                </span>
              ))}
            </span>
          }
        />
      )}
            {!p.meta_name && !p.meta_owner && !p.meta_summary && get(p, 'meta_tags.length', 0) === 0 && (
        <EmptyMessage>No metadata</EmptyMessage>
      )}
    </div>
  );
}

function MetaRow({ icon, label, value }: { icon: React.ReactNode; label: string; value: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', gap: 6, alignItems: 'flex-start' }}>
      <span style={{ color: 'var(--text-muted)', marginTop: 1 }}>{icon}</span>
      <span style={{ color: 'var(--text-secondary)', fontWeight: 500, flexShrink: 0 }}>{label}:</span>
      <span style={{ color: 'var(--text-primary)' }}>{value}</span>
    </div>
  );
}

function EmptyMessage({ children }: { children: React.ReactNode }) {
  return <div style={{ padding: 8, fontSize: 12, color: 'var(--text-muted)' }}>{children}</div>;
}
