import { useEffect, useMemo, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { callAddress, formsAheadOfFile, runAddressDecision, structuredSave, useStore, type SavePayloadSource } from '../../lib/store';
import { chainAddressSource } from '../../lib/address';
import { draftFileName, httpUrl, isHttpRequest, splitEndpoint } from '../../lib/http-endpoint';
import { useShallow } from 'zustand/react/shallow';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import { SHAPE_TONE } from '../../lib/shape';
import { groupSectionsByStep, sameAsFirst, sectionLines } from '../../lib/section-spans';
import ChainDiagram from './ChainDiagram';
import { planFacts, planHeadline, stepAsserts, stepSkips } from '../../lib/plan-headline';
import { runtimeRow, transportDrift } from '../../lib/plan-runtime';
import { aimsHttp } from '../../lib/env-address';
import { copyToClipboard } from 'luvo/data/clipboard';
import { Copy, TriangleAlert } from 'lucide-react';
import { useToast } from 'luvo/ui/useToast';
import { count } from 'luvo/data/plural';
import { addressOrigin, originClass } from '../../lib/plan-source';

type ExplainResponse = {
  documents: Plan[];
  runtime: { key: string; value: string; source: string }[][];
  sections: { section: string; start_line: number; end_line: number; content: string }[];
  mermaid: string;
  error: string | null;
};

type Plan = {
  file_path: string;
  connection: { address: string; source: string; backend: string };
  target: { endpoint: string; package: string | null; service: string | null; method: string | null };
  headers: { count: number } | null;
  requests: { skipped?: boolean }[];
  expectations: { skipped?: boolean; expectation_type?: string }[];
  assertions: { assertions?: string[]; skipped?: boolean }[];
  extractions: { skipped?: boolean; variables?: Record<string, string> }[];
  summary: {
    total_requests: number;
    total_responses: number;
    total_errors: number;
    error_expected: boolean;
    assertion_blocks: number;
    variable_extractions: number;
    rpc_mode_name: string;
  };
};

const TONE: Record<string, string> = {
  'Unary': SHAPE_TONE.unary,
  'Unary Error': SHAPE_TONE.unary,
  'Server Streaming': SHAPE_TONE.server,
  'Client Streaming': SHAPE_TONE.client,
  'Bidirectional Streaming': SHAPE_TONE.bidi,
};

type Lens = 'flow' | 'runtime' | 'sections';

const LENSES: { key: Lens; cli: string }[] = [
  { key: 'flow', cli: 'explain' },
  { key: 'runtime', cli: 'explain' },
  { key: 'sections', cli: 'inspect' },
];

export function PlanView() {
  const [lens, setLens] = useState<Lens>('flow');
  const toast = useToast();
  const rawContent = useStore(s => s.rawContent);
  const workspacePath = useStore(s => s.workspacePath);
  const draftName = useStore(s => draftFileName(s.workspacePath, s.request.endpoint));
  const documents = useStore(s => s.documents);
  const loadRawContent = useStore(s => s.loadRawContent);

  useEffect(() => { loadRawContent(); }, [loadRawContent, workspacePath]);

  const saveFrom = useStore(useShallow((s): SavePayloadSource => ({
    collectionParsed: s.collectionParsed,
    protocol: s.protocol,
    request: s.request,
    address: s.address,
    addressTouched: s.addressTouched,
    protocolTouched: s.protocolTouched,
  })));
  const hasFile = useStore(s => s.workspacePath !== null);
  const isHttp = useStore(s => isHttpRequest(s.workspacePath, s.request.endpoint));
  const ahead = useStore(formsAheadOfFile);
  const runTarget = useStore(useShallow(runAddressDecision));
  const executeTarget = useStore(callAddress);
  const protocol = useStore(s => s.protocol);
  const activeStep = useStore(s => s.activeStep);
  const setSectionKv = useStore(s => s.setSectionKv);
  const fileOptions = useStore(s => s.collectionParsed?.options ?? null);
  const draftJson = useMemo(() => (hasFile ? '' : JSON.stringify(structuredSave(saveFrom))), [hasFile, saveFrom]);
  const draft = useDebouncedPost<{ content: string }>(
    '/api/preview-structured',
    draftJson ? { ...JSON.parse(draftJson), path: draftName } : null,
    400,
  );

  const source = rawContent ?? draft.data?.content ?? null;
  const unsaved = workspacePath === null;

  const { data, error, busy } = useDebouncedPost<ExplainResponse>(
    '/api/explain',
    source ? { content: source, file_name: workspacePath ?? draftName } : null,
  );

  if (!source) {
    return <div className="empty-state">Pick a method and this says how the call would run.</div>;
  }

  const failure = error ?? data?.error ?? null;
  if (failure) {
    return <div className="assert is-fail"><span className="assert-mark">!</span><span>{failure}</span></div>;
  }

  const plans = data?.documents ?? [];
  if (plans.length === 0) {
    return <div className="empty-state">{busy || draft.busy ? 'Reading the file…' : 'Nothing to plan yet.'}</div>;
  }

  const fileName = workspacePath?.split('/').pop() ?? (isHttp ? 'file.httf' : 'file.gctf');
  const repeated = sameAsFirst(data?.runtime ?? []);
  const facts = documents.length > 0
    ? planFacts(
      documents,
      plans.map(p => p.summary.error_expected),
      plans.map(p => p.expectations.filter(e => !e.skipped && e.expectation_type !== 'error').length),
      plans.map(p => ({
        asserts: stepAsserts(p.assertions),
        variables: p.extractions
          .filter(e => !e.skipped)
          .reduce((n, e) => n + Object.keys(e.variables ?? {}).length, 0),
      })),
    )
    : null;

  return (
    <div className="stack">
      {facts && <div className="plan-headline mono">{planHeadline(facts)}</div>}
      {unsaved && (
        <div className="note">
          Nothing is saved yet — this is the file a save would write, planned as `explain` would read it.
        </div>
      )}
      {ahead && (
        <div className="note is-warn">
          The forms hold changes this plan has not seen: <span className="mono">explain</span> reads the
          file on disk, and so does Run. Save to plan what you typed.
        </div>
      )}
      <div className="bar">
        <Seg
          label="What to show about this file"
          value={lens}
          onChange={setLens}
          options={LENSES.map(l => ({ value: l.key, label: l.key }))}
        />
        <span className="grow" />
        <button
          className="btn is-ghost is-sm mono plan-cli"
          title="Copy this command"
          onClick={() => {
            const line = `grpctestify ${LENSES.find(l => l.key === lens)!.cli} ${fileName}`;
            void copyToClipboard(line)
              .then(() => toast.success('Command copied'))
              .catch(() => toast.error('The browser refused the clipboard'));
          }}
        >
          <Copy size={11} /> grpctestify {LENSES.find(l => l.key === lens)!.cli} {fileName}
        </button>
      </div>

      {lens === 'runtime' && (data?.runtime ?? []).map((options, i) => {
        const rows = options.map(runtimeRow);
        const drift = transportDrift(rows, protocol, fileOptions?.protocol);
        return (
        <div className="stack" key={i}>
          <span className="field-label">
            effective runtime — and where each value came from
            {plans.length > 1 && ` · step ${i + 1}`}
          </span>
          {repeated[i] && <span className="muted">Same as step 1.</span>}
          {!repeated[i] && (
          <>
          <div className="matches">
            {rows.map(o => (
              <div
                key={o.key}
                className={`match-row plan-runtime${o.fromFile ? ' is-set' : ''}${drift && o.key === 'protocol' ? ' is-drift' : ''}`}
              >
                <span className="mono plan-section-name">{o.key}</span>
                <span className="mono plan-runtime-value">{o.value}</span>
                <span className="grow plan-runtime-from">
                  {drift && o.key === 'protocol' ? 'default — the workbench sends over another' : o.from}
                </span>
              </div>
            ))}
          </div>
          {drift && (
            <div className="note is-warn plan-drift">
              <span className="grow">
                The workbench sends over <span className="mono">{drift.chosen}</span>, and nothing in the
                file says so: a run of it — here, and in CI — goes over <span className="mono">{drift.file}</span>.
              </span>
              {i === activeStep && (
                <button
                  className="btn is-sm"
                  onClick={() => {
                    setSectionKv('options', { ...(fileOptions ?? {}), protocol: drift.chosen });
                    toast.success('Written as OPTIONS.protocol — save to keep it');
                  }}
                >
                  write it as OPTIONS.protocol
                </button>
              )}
            </div>
          )}
          </>
          )}
        </div>
        );
      })}

      {lens === 'sections' && (
        <div className="stack">
          <span className="field-label">sections — what the parser saw</span>
          {groupSectionsByStep(data?.sections ?? []).map(group => (
            <div key={group.step}>
              {plans.length > 1 && <div className="rail-group">step {group.step}</div>}
              <div className="matches">
                {group.sections.map((s, i) => (
                  <div key={`${s.section}-${i}`} className="match-row plan-section">
                    <span className="mono plan-section-name">{s.section}</span>
                    <span className="muted mono plan-section-lines">{sectionLines(s)}</span>
                    <span className="muted grow">{s.content}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {lens === 'flow' && plans.map((plan, i) => {
        const inherited = !plan.connection.source.startsWith('ADDRESS');
        const doc = documents[i];
        const chained = inherited ? chainAddressSource(documents, i) : { address: '', from: -1 };
        const dials = inherited ? (chained.address || runTarget.address) : plan.connection.address;
        const http = plan.connection.backend === 'http' || plan.connection.backend === 'https';
        const path = http ? splitEndpoint(plan.target.endpoint).path : '';
        const aimedByEndpoint = /^https?:\/\//.test(path.trim());
        const url = http ? httpUrl(dials, path) : dials;
        const why = inherited
          ? (chained.address ? `the ADDRESS step ${chained.from + 1} names` : runTarget.why)
          : plan.connection.source;
        return (
          <fieldset key={i} className={`panel ${doc ? SHAPE_TONE[doc.kind] : ''}`}>
            <legend>step {i + 1}</legend>
            <div className="panel-body stack">
              <div className="bar">
                <span className={`badge is-kind ${TONE[plan.summary.rpc_mode_name] ?? SHAPE_TONE.unary}`}>
                  {plan.summary.rpc_mode_name}
                </span>
                <span className="mono grow">
                  {plan.target.endpoint.startsWith(`${plan.summary.rpc_mode_name} `)
                    ? plan.target.endpoint.slice(plan.summary.rpc_mode_name.length + 1)
                    : plan.target.endpoint}
                </span>
              </div>

              <dl className="kv">
                <dt>dials</dt>
                <dd className="mono">
                  {http ? url : dials}
                  <span className={originClass(addressOrigin({
                    own: !inherited,
                    fromChain: chained.address !== '',
                    source: runTarget.source,
                  }))}>
                    {why}
                  </span>
                  {http && url !== '' && !aimsHttp(url) && (
                    <span className="warn plan-warn">
                      <TriangleAlert size={9} /> an HTTP call needs a scheme — a run of this file is
                      refused before it dials
                    </span>
                  )}
                  {http && aimedByEndpoint && dials !== '' && (
                    <span className="muted plan-warn">
                      the ENDPOINT names the whole url, so this file's ADDRESS is not read
                    </span>
                  )}
                </dd>
                {inherited && executeTarget !== dials && (
                  <>
                    <dt>execute dials</dt>
                    <dd className="mono">
                      {executeTarget}
                      <span className="warn plan-warn">
                        <TriangleAlert size={9} /> the address in the header, which a run does not read
                      </span>
                    </dd>
                  </>
                )}
                <dt>over</dt>
                <dd className="mono">{plan.connection.backend}</dd>
                {plan.target.service && <><dt>service</dt><dd className="mono">{plan.target.service}</dd></>}
                {plan.target.method && <><dt>method</dt><dd className="mono">{plan.target.method}</dd></>}
                {plan.headers && plan.headers.count > 0 && (
                  <><dt>headers</dt><dd className="mono">{plan.headers.count}</dd></>
                )}
              </dl>

              <div className="bar wrap">
                <span className="badge">
                  {isHttp
                    ? plan.summary.total_requests === 0 ? 'no body' : 'body'
                    : `${count(plan.summary.total_requests, 'request')}`}
                </span>
                {plan.summary.total_responses > 0 && (
                  <span className="badge">{count(plan.summary.total_responses, 'expectation')}</span>
                )}
                {stepAsserts(plan.assertions) > 0 && (
                  <span className="badge is-ok">{count(stepAsserts(plan.assertions), 'assert')}</span>
                )}
                {plan.summary.variable_extractions > 0 && (
                  <span className="badge is-info">{count(plan.summary.variable_extractions, 'extraction')}</span>
                )}
                {plan.summary.error_expected && <span className="badge is-fail">expects an error</span>}
                {stepSkips(plan).length > 0 && (
                  <span
                    className="badge is-pending"
                    title={`#[skip] on ${stepSkips(plan).join(', ')} — the counts beside this are what the file holds, not what a run does`}
                  >
                    {stepSkips(plan).join(' · ')} skipped
                  </span>
                )}
              </div>

              {doc && doc.produces.length > 0 && (
                <div className="bar wrap">
                  <span className="field-label">passes on</span>
                  {doc.produces.map(v => <span key={v} className="chip is-on mono">{v}</span>)}
                </div>
              )}
            </div>
          </fieldset>
        );
      })}

      {lens === 'flow' && plans.length > 1 && documents.length > 1 && (
        <ChainDiagram
          documents={documents}
          summaries={plans.map(p => ({
            ...p.summary,
            running: {
              checks: stepAsserts(p.assertions)
                + p.expectations.filter(e => !e.skipped && e.expectation_type !== 'error').length,
              binds: p.extractions
                .filter(e => !e.skipped)
                .flatMap(e => Object.keys(e.variables ?? {})),
            },
          }))}
        />
      )}
      {lens === 'flow' && data?.mermaid && plans.length > 1 && (
        <details className="mermaid-block">
          <summary>mermaid — the same diagram as source, for a PR or a doc</summary>
          <div className="bar">
            <span className="grow" />
            <button
              className="btn is-sm is-ghost"
              onClick={() => void copyToClipboard(`\`\`\`mermaid\n${data.mermaid}\n\`\`\``).catch(() => {})}
            >
              <Copy size={11} /> copy
            </button>
          </div>
          <pre className="diff">{data.mermaid}</pre>
        </details>
      )}
      {lens === 'flow' && plans.length > 1 && (
        <div className="note">
          The chain runs head to tail and stops at the first failure — a later step never runs.
          {documents.some(d => d.parallel) && ' Steps marked parallel go out together as one group: every one of them finishes whatever the others do, and the chain stops after the group.'}
          {' '}The EXTRACT variables cross a step boundary, and so does the last ADDRESS named for a
          step&apos;s own transport; headers, TLS, PROTO and OPTIONS do not.
        </div>
      )}
    </div>
  );
}
