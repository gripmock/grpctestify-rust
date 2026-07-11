import Editor from '@monaco-editor/react';
import get from 'lodash-es/get';
import { useStore } from '../../lib/store';
import { Tag, User, FileText, Shield, Wrench } from 'lucide-react';
import type { CollectionParsed } from '../../lib/types';
import { ProtoManager } from './ProtoManager';

export function GctfInfoPanel() {
  const p = useStore(s => s.collectionParsed);
  const tab = useStore(s => s.gctfTab);
  const setTab = useStore(s => s.setGctfTab);

  if (!p) return null;

  const tabs: { key: typeof tab; label: string; count?: number }[] = [
    { key: 'request', label: 'Config' },
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
      {tab === 'asserts' && <AssertsView asserts={p.asserts || []} />}
      {tab === 'extracts' && <ExtractsView extracts={p.extracts || {}} />}
      {tab === 'meta' && <MetaView p={p} />}
      {tab === 'proto' && <ProtoManager />}
    </section>
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
