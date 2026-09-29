import { useMemo } from 'react';
import { useStore } from '../../lib/store';
import { missingPaths } from '../../lib/assert-problems';
import { Seg } from 'luvo/ui/Seg';
import { pathPlaceholderNote } from '../../lib/path-placeholder';
import { TLS_ALIASES, TLS_MODES, aliasValue, applyTlsMode, setAlias, tlsModeOf, unknownKeys } from '../../lib/section-model';
import { caUnused, halfIdentity } from '../../lib/tls-shape';

const PATHS: [string, string][] = [
  ['ca_cert', 'CA certificate'],
  ['client_cert', 'Client certificate'],
  ['client_key', 'Client key'],
  ['server_name', 'SNI server name'],
];

const KNOWN = ['insecure', ...Object.values(TLS_ALIASES).flat()];

export function TlsEditor() {
  const parsed = useStore(s => s.collectionParsed);
  const setSectionKv = useStore(s => s.setSectionKv);
  const tls = parsed?.tls ?? {};
  const mode = tlsModeOf(tls);

  const certNote = pathPlaceholderNote(...PATHS.map(([key]) => aliasValue(tls, TLS_ALIASES[key])));
  const diagnostics = useStore(s => s.diagnostics);
  const gone = useMemo(() => missingPaths(diagnostics, 'TLS'), [diagnostics]);
  const caIgnored = caUnused(tls);
  const missing = halfIdentity(tls);

  return (
    <div className="stack">
      <div className="bar">
        <span className="field-label">transport security</span>
        <Seg
          label="Transport security"
          value={mode}
          onChange={m => setSectionKv('tls', applyTlsMode(tls, m))}
          options={TLS_MODES.map(m => ({ value: m, label: m }))}
        />
      </div>

      <div className="note">
        <span className="mono">plaintext</span> writes no TLS section at all ·
        <span className="mono"> tls</span> verifies the chain ·
        <span className="mono"> insecure</span> is TLS with verification skipped, written as
        <span className="mono"> insecure: true</span>.
      </div>

      {gone.map(({ named, at }, i) => (
        <div key={`gone-${i}`} className="note is-warn">
          <span className="mono">{named}</span> is not there
          {at !== null && <> — the workbench looked in <span className="mono">{at}</span></>}.
        </div>
      ))}

      <div className="editor-frame stack">
        {PATHS.map(([key, label]) => {
          const dead = key === 'ca_cert' && caIgnored;
          const wanted = missing === key;
          return (
          <label key={key} className={`stack cert-field${dead ? ' is-overruled' : ''}`}
            title={dead ? 'insecure: true skips verification, so no CA is read' : undefined}>
            <span className="field-label">
              {label}
              {dead && <span className="muted"> · not used</span>}
              {wanted && <span className="muted"> · needed</span>}
            </span>
            <input
              className="field mono"
              disabled={mode === 'plaintext'}
              placeholder={key === 'server_name' ? 'api.internal' : `/path/to/${key}.pem`}
              value={aliasValue(tls, TLS_ALIASES[key])}
              onChange={e => setSectionKv('tls', setAlias(tls, TLS_ALIASES[key], e.target.value))}
            />
          </label>
          );
        })}
        <div className="muted cert-note">Paths resolve on the server, relative to this file.</div>
        {missing && (
          <div className="note is-warn">
            A client identity is the certificate and the key together. With only the
            <span className="mono"> {missing === 'client_key' ? 'certificate' : 'key'}</span>, a gRPC call
            dials with no identity at all and the server answers <span className="mono">unauthenticated</span>;
            over grpc-web the client refuses to build.
          </div>
        )}
        {caIgnored && (
          <div className="note">
            <span className="mono">insecure: true</span> replaces the verifier, so the CA above is never
            read — the call trusts any certificate. Switch to <span className="mono">tls</span> for the CA
            to mean anything.
          </div>
        )}
        {certNote && <div className="note is-warn">{certNote}</div>}
      </div>

      {unknownKeys(tls, KNOWN).length > 0 && (
        <div>
          <div className="field-label">also in this section</div>
          <div className="bar wrap">
            {unknownKeys(tls, KNOWN).map(([k, v]) => (
              <span key={k} className="chip mono">{k}: {v}</span>
            ))}
          </div>
          <div className="muted">Kept as written; edit them in the source tab.</div>
        </div>
      )}
    </div>
  );
}
