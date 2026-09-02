import { useCallback, useEffect, useState } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { Seg } from 'luvo/ui/Seg';
import { effectiveTls, runAddressDecision, useStore } from '../../lib/store';
import { durationLabel, timeoutSeconds } from '../../lib/format';
import { addressDecision, runDivergence } from '../../lib/address';
import { isHttpRequest } from '../../lib/http-endpoint';
import { switchable } from '../../lib/call-kind';
import { pathTail } from '../../lib/path-tail';
import { defaultAddressFor } from '../../lib/types';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import { ChevronDown, KeyRound, Save, TriangleAlert } from 'lucide-react';
import { useToast } from 'luvo/ui/useToast';
import { compressionFromFile, connectionFromFile, connectionUsed, fileConnectionNote, timeoutUsed } from '../../lib/connection-source';
import { healthNote, probeTarget, type TargetHealth } from '../../lib/target-health';

const PROTOCOLS = [
  { value: 'grpc' as const, label: 'grpc' },
  { value: 'grpc-web' as const, label: 'grpc-web' },
  { value: 'connectrpc' as const, label: 'connect', short: 'connect' },
];

export type TlsMode = 'plaintext' | 'tls' | 'insecure';

const TLS_MODES = [
  { value: 'plaintext' as const, label: 'plaintext', hint: 'No TLS' },
  { value: 'tls' as const, label: 'tls', hint: 'TLS with certificate verification' },
  { value: 'insecure' as const, label: 'insecure', hint: 'TLS, verification skipped' },
];

export function ConnectionPopover() {
  const protocol = useStore(s => s.protocol);
  const setProtocol = useStore(s => s.setProtocol);
  const tls = useStore(s => s.tls);
  const setTls = useStore(s => s.setTls);
  const tlsInsecure = useStore(s => s.tlsInsecure);
  const setTlsInsecure = useStore(s => s.setTlsInsecure);
  const tlsCa = useStore(s => s.tlsCa);
  const setTlsCa = useStore(s => s.setTlsCa);
  const tlsCert = useStore(s => s.tlsCert);
  const setTlsCert = useStore(s => s.setTlsCert);
  const tlsKey = useStore(s => s.tlsKey);
  const setTlsKey = useStore(s => s.setTlsKey);
  const requestTimeoutMs = useStore(s => s.requestTimeoutMs);
  const setRequestTimeoutMs = useStore(s => s.setRequestTimeoutMs);

  const parsed = useStore(s => s.collectionParsed);
  const fromFile = connectionFromFile(parsed, { protocol, tls, tlsInsecure });

  const waits = timeoutUsed(parsed, requestTimeoutMs);
  const waitLabel = durationLabel(waits.seconds * 1000);
  const compresses = compressionFromFile(parsed);
  const fileNote = [
    fileConnectionNote(fromFile),
    waits.source === 'file'
      ? waits.from === 'attribute'
        ? `#[timeout(${waits.seconds})]`
        : `OPTIONS timeout: ${waits.seconds}s`
      : '',
    compresses ? `OPTIONS compression: ${compresses}` : '',
  ].filter(Boolean).join(' · ');
  const diverged = !!fromFile.protocol?.differs
    || !!fromFile.tls?.differs
    || (waits.source === 'file' && waits.seconds !== timeoutSeconds(requestTimeoutMs));

  const [open, setOpen] = useState(false);
  const [savingDefault, setSavingDefault] = useState(false);
  const defaults = useStore(st => st.projectDefaults);
  const projectRoot = useStore(st => st.projectRoot);
  const address = useStore(st => st.address);
  const toast = useToast();
  const ref = useDismiss<HTMLDivElement>(open, useCallback(() => setOpen(false), []));

  const tlsMode: TlsMode = !tls ? 'plaintext' : tlsInsecure ? 'insecure' : 'tls';
  const setTlsMode = (mode: TlsMode) => {
    setTls(mode !== 'plaintext');
    setTlsInsecure(mode === 'insecure');
  };

  const hasCerts = !!(tlsCa || tlsCert || tlsKey);

  const fileAddress = useStore(st => st.collectionParsed?.address ?? null);
  const typedAddress = useStore(st => st.address);
  const serverAddress = useStore(st => st.serverEnv.address ?? null);
  const activeEnvName = useStore(st => st.activeEnvironment);
  const environmentAddress = useStore(st => (st.activeEnvironment
    ? st.environments.find(e => e.name === st.activeEnvironment)?.address ?? null
    : null));
  const isHttp = useStore(st => isHttpRequest(st.workspacePath, st.request.endpoint));
  const setCallKind = useStore(st => st.setCallKind);
  const openFile = useStore(st => st.workspacePath);
  const canSwitch = switchable(openFile);
  const decision = addressDecision({
    file: fileAddress,
    typed: typedAddress,
    environment: environmentAddress,
    server: serverAddress,
    fallback: isHttp ? '' : defaultAddressFor(protocol),
  });
  const runTarget = useStore(useShallow(runAddressDecision));
  const hasFile = useStore(st => st.workspacePath !== null);
  const runElsewhere = runDivergence(decision, runTarget, hasFile);
  const ORDER: { source: 'file' | 'typed' | 'environment' | 'server' | 'default'; name: string; value: string; note?: string }[] = [
    { source: 'file' as const, name: 'this file', value: fileAddress ?? '' },
    { source: 'typed' as const, name: 'the header', value: typedAddress.trim(), note: 'execute only' },
    { source: 'environment' as const, name: activeEnvName ? `env: ${activeEnvName}` : 'the environment', value: environmentAddress ?? '' },
    { source: 'server' as const, name: 'GRPCTESTIFY_ADDRESS', value: serverAddress ?? '' },
  ];
  if (!isHttp) {
    ORDER.push({ source: 'default' as const, name: `${protocol} default`, value: defaultAddressFor(protocol) });
  }
  const scheme = decision.address.trim().startsWith('https://') ? 'https' : 'http';
  const client = useStore(useShallow(effectiveTls));
  const used = connectionUsed(parsed, { protocol, tls: client.tls, tlsInsecure: client.tlsInsecure });
  const usedMode: TlsMode = !used.tls ? 'plaintext' : used.tlsInsecure ? 'insecure' : 'tls';
  const protocolLabel = isHttp
    ? scheme
    : PROTOCOLS.find(p => p.value === used.protocol)?.label ?? used.protocol;

  const [probe, setProbe] = useState<{ target: string; asked: number; health: TargetHealth | null } | null>(null);
  const target = decision.address.trim();
  const [asked, setAsked] = useState(0);
  useEffect(() => {
    if (!open || target === '') { return; }
    let live = true;
    void probeTarget(target).then(found => {
      if (live) setProbe({ target, asked, health: found });
    });
    return () => { live = false; };
  }, [open, target, asked]);
  const answered = probe !== null && probe.target === target && probe.asked === asked;
  const health = answered ? probe.health : null;
  const probing = open && target !== '' && !answered;

  const matchesProject = !!defaults
    && defaults.address === address.trim()
    && defaults.protocol === protocol
    && defaults.tls === tls
    && defaults.tlsInsecure === tlsInsecure
    && defaults.activeEnv === activeEnvName;

  return (
    <div ref={ref} className="picker">
      <button
        className={`btn is-sm is-quiet conn-chip${isHttp ? ' is-derived' : ''}${open ? ' is-focus' : ''}${runElsewhere ? ' is-warn' : ''}`}
        onClick={() => setOpen(v => !v)}
        aria-haspopup="dialog"
        aria-expanded={open}
        title={[
          isHttp
            ? `${protocolLabel} · ${waitLabel} — the scheme of the address decides the transport`
            : `${protocolLabel} · ${usedMode} · ${waitLabel} — transport, security and timeout for this request`,
          waits.source === 'file'
            ? `This file waits ${waitLabel}: its own ${waits.from === 'attribute' ? 'section attribute' : 'OPTIONS timeout'}, which wins over the workbench's`
            : waits.source === 'default'
              ? `No timeout is set here or in the file, so a call gives up after ${waitLabel}`
              : `A call gives up after ${waitLabel}`,
          fileNote && `This file carries its own: ${fileNote}${diverged ? ' — calls from it use those, not this' : ''}`,
          runElsewhere
            && `A run of this file dials ${runElsewhere.address || 'nowhere'} — ${runElsewhere.why}. \`run\` never reads the header.`,
        ].filter(Boolean).join('\n')}
      >
        <span className={`mono${!isHttp && fromFile.protocol ? ' is-derived' : ''}`}>{protocolLabel}</span>
        {!isHttp && (
          <>
            <span className="conn-sep conn-fold">·</span>
            <span
              className={`mono conn-fold${usedMode === 'insecure' ? ' warn' : ''}${fromFile.tls ? ' is-derived' : ''}`}
            >
              {usedMode}
            </span>
            {hasCerts && <KeyRound size={10} />}
          </>
        )}
        <span className="conn-sep conn-fold-late">·</span>
        <span className={`mono conn-fold-late${waits.source === 'file' ? ' is-derived' : ''}`}>{waitLabel}</span>
        {compresses && (
          <>
            <span className="conn-sep conn-fold-late">·</span>
            <span className="mono conn-fold-late is-derived">{compresses}</span>
          </>
        )}
        {diverged && <span className="badge is-pending conn-file">file</span>}
        <ChevronDown size={11} />
      </button>

      <Popover open={open} anchor={ref} align="end" className="conn-panel">
        <div className="menu">
          <div className="stack">
            {fileNote && (
              <div className={`note${diverged ? ' is-warn' : ''}`}>
                This file carries its own connection — <span className="mono">{fileNote}</span>.
                {diverged && ' Calls from this file use those; what is set here applies to the rest.'}
                {fromFile.protocol?.differs && (
                  <button className="btn is-sm is-ghost" onClick={() => setProtocol(fromFile.protocol!.value)}>
                    use {fromFile.protocol.value}
                  </button>
                )}
                {fromFile.tls?.differs && (
                  <button className="btn is-sm is-ghost" onClick={() => setTlsMode(fromFile.tls!.value)}>
                    use {fromFile.tls.value}
                  </button>
                )}
              </div>
            )}
            <div className="stack conn-field">
              <span className="field-label">where calls go</span>
              <div className="conn-order">
                {ORDER.map(step => (
                  <div key={step.source} className={`conn-step${decision.source === step.source ? ' is-on' : ''}`}>
                    <span className="conn-step-name">{step.name}</span>
                    <span className="mono conn-step-value" title={step.value || undefined}>
                      {step.value || <span className="muted">—</span>}
                    </span>
                    {step.note && <span className="muted conn-step-note">{step.note}</span>}
                  </div>
                ))}
              </div>
              <span className="muted cert-note">
                {decision.address
                  ? <>Calls go to <span className="mono">{decision.address}</span> — {decision.why}.</>
                  : decision.why.charAt(0).toUpperCase() + decision.why.slice(1) + '.'}
              </span>
              {target !== '' && (
                <span className={`bar conn-health${health && !health.reachable && !probing ? ' is-cold' : ''}`}>
                  <span className={`dot${probing ? '' : health?.reachable ? ' is-ok' : health ? ' is-fail' : ''}`} />
                  <span className="muted grow">{healthNote(health, probing)}</span>
                  <button className="btn is-ghost is-sm" onClick={() => setAsked(n => n + 1)} disabled={probing} title="Open a socket there again">
                    try again
                  </button>
                </span>
              )}
              {runElsewhere && (
                <div className="note is-warn conn-run-note">
                  <TriangleAlert size={11} />
                  <span>
                    A run of this file dials{' '}
                    {runElsewhere.address
                      ? <><span className="mono">{runElsewhere.address}</span> — {runElsewhere.why}</>
                      : <>nowhere — {runElsewhere.why}</>}
                    . `run` never reads the header, here or in a terminal.
                  </span>
                </div>
              )}
            </div>

            <div className="stack conn-field">
              <span className="field-label">what this calls</span>
              <Seg
                label="Call kind"
                value={isHttp ? 'http' : 'grpc'}
                onChange={value => setCallKind(value as 'grpc' | 'http')}
                options={[
                  {
                    value: 'grpc',
                    label: 'gRPC',
                    title: canSwitch.can ? 'A service and a method' : canSwitch.why,
                    disabled: !canSwitch.can && isHttp,
                  },
                  {
                    value: 'http',
                    label: 'HTTP',
                    title: canSwitch.can ? 'A method and a path' : canSwitch.why,
                    disabled: !canSwitch.can && !isHttp,
                  },
                ]}
              />
              {!canSwitch.can && <span className="muted">{canSwitch.why}</span>}
            </div>

            {isHttp && (
              <div className="note">
                An HTTP file carries its own scheme: <span className="mono">http://</span> or
                <span className="mono"> https://</span> in the address decides the transport and its
                security. The timeout below still applies.
              </div>
            )}

            {!isHttp && (
            <div className="stack conn-field">
              <span className="field-label">wire protocol</span>
              <Seg
                label="Protocol"
                value={protocol}
                onChange={setProtocol}
                options={PROTOCOLS.map(p => ({ value: p.value, label: p.label }))}
              />
            </div>
            )}

            {!isHttp && (
            <div className="stack conn-field">
              <span className="field-label">transport security</span>
              <Seg
                label="Transport security"
                value={tlsMode}
                onChange={setTlsMode}
                options={TLS_MODES.map(m => ({ value: m.value, label: m.label, title: m.hint }))}
              />
            </div>
            )}

            {!isHttp && (
            <div className="stack conn-field">
              <span className="field-label">client certificate (mTLS)</span>
              {([
                ['CA cert path', tlsCa, setTlsCa, '/path/to/ca.pem'],
                ['Client cert path', tlsCert, setTlsCert, '/path/to/client.pem'],
                ['Client key path', tlsKey, setTlsKey, '/path/to/client-key.pem'],
              ] as const).map(([label, value, set, placeholder]) => (
                <div key={label} className="field-frame" title={label}>
                  <input
                    className="field mono"
                    value={value}
                    disabled={!tls}
                    onChange={e => set(e.target.value)}
                    placeholder={placeholder}
                  />
                </div>
              ))}
              <span className="muted cert-note">
                {tls
                  ? 'Paths resolve on the server; nothing is uploaded from the browser.'
                  : 'Switch to tls or insecure to send a client certificate.'}
              </span>
            </div>
            )}

            {projectRoot && (
              <div className="stack conn-field">
                <span className="field-label">project default</span>
                <button
                  className="btn is-sm"
                  disabled={savingDefault || address.trim() === '' || matchesProject}
                  onClick={async () => {
                    setSavingDefault(true);
                    try {
                      const ok = await useStore.getState().saveProjectSettings({
                        address: address.trim(),
                        protocol,
                        tls,
                        tls_insecure: tlsInsecure,
                        active_env: activeEnvName,
                      });
                      if (ok) toast.success(`${pathTail(`${projectRoot}/settings.json`)} now starts at ${address.trim()}`);
                      else toast.error('The workbench could not write settings.json');
                    } finally {
                      setSavingDefault(false);
                    }
                  }}
                  title={address.trim() === ''
                    ? 'Type an address first — the project default is a target, not an empty field'
                    : matchesProject
                      ? 'This is already what the project starts with'
                      : `Write ${address.trim()}${isHttp ? '' : `, ${protocol}, ${tlsMode}`}${activeEnvName ? ` and the "${activeEnvName}" environment` : ''} into the project`}
                >
                  <Save size={11} /> save as project default
                </button>
                <span className="muted cert-note">
                  {!defaults
                    ? 'Where a session starts when nothing else aims it. This file is shared with the project.'
                    : matchesProject
                      ? `This session is what the project starts with. The file is shared with the project.`
                      : `The project starts at ${defaults.address || 'no address'} — this session is somewhere else.`}
                </span>
                {defaults && !matchesProject && (
                  <button
                    className="btn is-sm is-ghost"
                    onClick={() => {
                      const st = useStore.getState();
                      st.setAddress(defaults.address);
                      st.setProtocol(defaults.protocol);
                      st.setTls(defaults.tls);
                      st.setTlsInsecure(defaults.tlsInsecure);
                      if (defaults.activeEnv !== activeEnvName) st.setActiveEnvironment(defaults.activeEnv);
                      toast.success(`Back to what the project starts with — ${defaults.address || 'no address'}`);
                    }}
                    title="Set this session back to the project's own connection"
                  >
                    use the project's
                  </button>
                )}
              </div>
            )}

            <div className="stack conn-field">
              <span className="field-label">request timeout</span>
              <div className="field-frame timeout-field">
                <input
                  className="field mono"
                  type="number"
                  min={0}
                  value={timeoutSeconds(requestTimeoutMs)}
                  onChange={e => setRequestTimeoutMs(Math.max(0, parseInt(e.target.value) || 0) * 1000)}
                  placeholder="0"
                />
                <span className="field-label">s</span>
              </div>
              <span className="muted cert-note">
                {waits.source === 'file'
                  ? <>This file waits <span className="mono">{waitLabel}</span> — its own <span className="mono">{waits.from === 'attribute' ? `#[timeout(${waits.seconds})]` : 'OPTIONS timeout'}</span> wins over this box.</>
                  : waits.source === 'default'
                    ? <>0 is not “wait forever”: a call with no timeout of its own gives up after <span className="mono">30 s</span>.</>
                    : <>A file’s own <span className="mono">OPTIONS timeout</span> wins over this.</>}
              </span>
            </div>
          </div>
        </div>
      </Popover>
    </div>
  );
}
