import { useMemo, useState } from 'react';
import { useStore } from '../../lib/store';
import { Plus, X, TriangleAlert, Info, Eye, EyeOff } from 'lucide-react';
import { EnvVarToolbar } from './EnvVarToolbar';
import { effectiveEnvironment, substituteEnv } from '../../lib/env';
import { checkMetadataKey, checkMetadataValue } from '../../lib/metadata';
import { isHttpRequest } from '../../lib/http-endpoint';
import { hidesTyped, isSecretHeader, splitScheme, variableNameFor } from '../../lib/secret-headers';
import { knownHeaders } from '../../lib/known-headers';
import { droppedLines } from '../../lib/assert-problems';

export function HeadersEditor() {
  const request = useStore(s => s.request);
  const isHttp = useStore(s => isHttpRequest(s.workspacePath, s.request.endpoint));
  const wire = isHttp ? 'http' : 'grpc';
  const [shown, setShown] = useState<Set<string>>(new Set());
  const setRequestHeaders = useStore(s => s.setRequestHeaders);
  const openEnvManager = useStore(s => s.openEnvManager);
  const activeEnv = useStore(s => {
    const ae = s.activeEnvironment;
    return ae ? s.environments.find(e => e.name === ae) : null;
  });
  const env = useMemo(() => effectiveEnvironment(activeEnv), [activeEnv]);

  const diagnostics = useStore(s => s.diagnostics);
  const dropped = useMemo(() => droppedLines(diagnostics, 'REQUEST_HEADERS'), [diagnostics]);

  const entries = Object.entries(request.headers);
  const known = knownHeaders(wire);
  const hasUnnamed = entries.some(([k]) => k === '');

  const set = (key: string, value: string, oldKey?: string) => {
    const h = { ...request.headers };
    if (oldKey !== undefined && oldKey !== key) delete h[oldKey];
    if (key) h[key] = value;
    else delete h[oldKey!];
    setRequestHeaders(h);
  };

  const add = () => {
    setRequestHeaders({ ...request.headers, '': '' });
  };

  const allValues = entries.map(([, v]) => v).join('\n');

  return (
    <div>
      <datalist id={`known-headers-${wire}`}>
        {known.map(name => <option key={name} value={name} />)}
      </datalist>
      <div className="editor-frame stack">
        {dropped.map((line, i) => (
          <div key={`dropped-${i}`} className="assert is-fail">
            <span className="assert-mark"><TriangleAlert size={12} /></span>
            <span className="stack is-tight">
              <span className="mono">{line}</span>
              <span className="assert-said">
                This line is not a <span className="mono">key: value</span> pair, so the file drops
                it and the call goes out without it.
              </span>
            </span>
          </div>
        ))}
        {entries.length === 0 && dropped.length === 0 && (
          <div className="muted">
            Sent {isHttp ? 'as request headers' : 'as gRPC metadata'} — a value takes{' '}
            <span className="mono">{'{{VAR}}'}</span> from the environment.
          </div>
        )}

        {entries.map(([k, v], i) => {
          const resolved = substituteEnv(v, env);
          const isDifferent = resolved !== v;
          const note = checkMetadataKey(k, wire) ?? checkMetadataValue(k, resolved, wire);
          const secret = isSecretHeader(k);
          const hidden = hidesTyped(k, v) && !shown.has(k);
          const visible = shown.has(k);
          const secretResolved = secret && !visible;
          return (
            <div key={i} className={`kvrow${note ? ` is-${note.level}` : ''}`}>
              <span className="kv-mark" title={note?.reason}>
                {note?.level === 'bad' ? <TriangleAlert size={11} />
                  : note?.level === 'note' ? <Info size={11} />
                  : null}
              </span>
              <input
                className="field field-frame mono"
                value={k}
                onChange={e => set(e.target.value, v, k)}
                placeholder="key"
                list={`known-headers-${wire}`}
                spellCheck={false}
              />
              <span className={`field-frame${v.includes('{{') && env ? ' is-templated' : ''}`}>
                <input
                  className="field mono"
                  type={hidden ? 'password' : 'text'}
                  autoComplete="off"
                  value={v}
                  onChange={e => set(k, e.target.value, k)}
                  placeholder="value"
                  spellCheck={false}
                  title={isDifferent && !secretResolved ? `Goes out as: ${resolved}` : undefined}
                />
                {isDifferent && !secretResolved && (
                  <span className="badge is-info mono" title={resolved}>{resolved}</span>
                )}
                {hidesTyped(k, v) && v.trim() !== '' && (
                  <button
                    className="btn is-ghost is-sm"
                    onClick={() => {
                      const { prefix, secret: credential } = splitScheme(v);
                      const name = variableNameFor(k);
                      set(k, `${prefix}{{${name}}}`, k);
                      openEnvManager(name, credential);
                    }}
                    title={`Move this value into {{${variableNameFor(k)}}} — the file keeps the name, the environment keeps the value, and a credential-shaped name is kept out of git`}
                  >
                    keep it in the environment
                  </button>
                )}
                {secret && (
                  <button
                    className="btn is-ghost is-icon is-sm"
                    onClick={() => setShown(prev => {
                      const next = new Set(prev);
                      if (!next.delete(k)) next.add(k);
                      return next;
                    })}
                    title={visible ? `Hide the ${k} value` : `Show the ${k} value`}
                    aria-label={visible ? 'Hide value' : 'Show value'}
                  >
                    {visible ? <EyeOff size={11} /> : <Eye size={11} />}
                  </button>
                )}
              </span>
              <button className="btn is-ghost is-icon" onClick={() => set('', '', k)} aria-label={`Remove ${k || 'header'}`}>
                <X size={14} />
              </button>
              {note && <span className="kv-why">{note.reason}</span>}
            </div>
          );
        })}

        <button
          className="btn is-quiet add-row"
          onClick={add}
          disabled={hasUnnamed}
          title={hasUnnamed ? 'Name the empty header first' : isHttp ? 'One more header' : 'One more metadata pair'}
        >
          <Plus size={12} /> Add header
        </button>
      </div>
      <EnvVarToolbar text={allValues} />
    </div>
  );
}
