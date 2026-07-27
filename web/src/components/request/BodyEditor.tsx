import { useMemo, useState } from 'react';
import { MonacoEditor as Editor } from '../MonacoEditor';
import { useStore, effectiveTls } from '../../lib/store';
import { useShallow } from 'zustand/react/shallow';
import { btn, colors } from '../../lib/theme';
import { Plus, X, Layers, Send, Sparkles, Loader2 } from 'lucide-react';
import { EnvVarToolbar } from './EnvVarToolbar';
import { registerEnvHoverProvider, addEnvDecorations } from '../../lib/monaco-env-hover';

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
  const monacoTheme = useStore(s => s.theme === 'dark' ? 'vs-dark' : 'light');
  const address = useStore(s => s.address);
  const protocol = useStore(s => s.protocol);
  const selectedCollection = useStore(s => s.selectedCollection);
  const { tls, tlsInsecure, tlsCa, tlsCert, tlsKey } = useStore(useShallow(effectiveTls));

  const activeEnv = useStore(s => {
    const ae = s.activeEnvironment;
    return ae ? s.environments.find(e => e.name === ae) : null;
  });

  const reflectionMethods = useStore(s => s.reflectionMethods);
  const method = useMemo(
    () => reflectionMethods.find(m => m.fullName === request.endpoint),
    [reflectionMethods, request.endpoint]
  );

  const isMulti = request.bodies.length > 1;
  const shape = method
    ? method.clientStreaming && method.serverStreaming ? 'Bidirectional streaming'
    : method.clientStreaming ? 'Client streaming'
    : method.serverStreaming ? 'Server streaming'
    : 'Unary'
    : isMulti ? 'Client streaming' : 'Unary';
  const shapeMismatch = method && !method.clientStreaming && isMulti;

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
        body: JSON.stringify({
          address,
          endpoint: request.endpoint,
          tls: tls || undefined,
          tls_insecure: tls ? tlsInsecure : undefined,
          tls_ca: tls ? (tlsCa || undefined) : undefined,
          tls_cert: tls ? (tlsCert || undefined) : undefined,
          tls_key: tls ? (tlsKey || undefined) : undefined,
          collection_path: selectedCollection || undefined,
          protocol: protocol || undefined,
        }),
      });
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

  return (
    <div>
      <style>{`
        .env-var-active { background: #22c55e22; border-radius: 3px; border-bottom: 2px solid #22c55e; cursor: pointer; }
        .env-var-secret { background: #f59e0b22; border-radius: 3px; border-bottom: 2px solid #f59e0b; cursor: pointer; }
        .env-var-muted { background: #ef444422; border-radius: 3px; border-bottom: 2px solid #ef4444; cursor: pointer; text-decoration: line-through; opacity: 0.6; }
        .env-var-unknown { background: #6b728022; border-radius: 3px; border-bottom: 2px dashed #6b7280; cursor: pointer; }
      `}</style>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6, fontSize: 11, color: 'var(--text-muted)' }}>
        <Layers size={12} />
        <span>{request.bodies.length} message{request.bodies.length !== 1 ? 's' : ''}</span>
        <span style={{ fontSize: 10, color: shapeMismatch ? colors.warning : 'var(--text-muted)', display: 'flex', alignItems: 'center', gap: 3 }}>
          <Send size={10} /> {shape}{shapeMismatch ? ' — method is unary, extra messages are sent as separate calls' : ''}
        </span>
        <span style={{ flex: 1 }} />
        <button onClick={addRequestBody} style={btn('ghost', 'sm')}>
          <Plus size={12} /> Add message
        </button>
      </div>

      {request.bodies.map((body, idx) => (
        <div key={idx} style={{ marginBottom: 8 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
            <span style={{
              fontSize: 10, fontWeight: 600, color: 'var(--text-secondary)',
              background: 'var(--bg-tertiary)', borderRadius: 4,
              padding: '1px 6px', fontFamily: 'monospace',
            }}>
              #{idx + 1}
            </span>
            {isMulti && (
              <button onClick={() => removeRequestBody(idx)} style={btn('ghost', 'sm')}>
                <X size={11} />
              </button>
            )}
            <span style={{ flex: 1 }} />
            {fillErrors[idx] && (
              <span style={{ fontSize: 10, color: colors.error, maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {fillErrors[idx]}
              </span>
            )}
            <button onClick={() => handleAutoFill(idx)} disabled={!request.endpoint || fillingIdxs.has(idx)} style={{
              ...btn('ghost', 'sm'),
              opacity: request.endpoint && !fillingIdxs.has(idx) ? 1 : 0.4,
              cursor: request.endpoint && !fillingIdxs.has(idx) ? 'pointer' : 'not-allowed',
            }} title="Auto-fill this message from the proto schema">
              {fillingIdxs.has(idx) ? <Loader2 size={12} className="animate-spin" /> : <Sparkles size={12} />}
              Auto Fill
            </button>
          </div>
          <div style={{ border: '1px solid var(--border)', borderRadius: 6, overflow: 'hidden' }}>
            <Editor
              height="180px"
              language="json"
              value={body}
              onChange={v => setRequestBody(idx, v || '')}
              theme={monacoTheme}
              onMount={(ed, monaco) => {
                ensureJsonFormatter(monaco);
                registerEnvHoverProvider(monaco, () => activeEnv);
                addEnvDecorations(ed, monaco, () => activeEnv);
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
      ))}
    </div>
  );
}
