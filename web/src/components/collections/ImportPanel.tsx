import { useEffect, useState } from 'react';
import { useStore } from '../../lib/store';
import { parseShell } from '../../lib/shell';
import { importSummary, planImport } from '../../lib/grpcurl-import';
import { curlSummary, isCurl, parseCurl } from '../../lib/curl-import';
import { callSummary, grpctestifySubcommand, isGrpctestify, parseGrpctestifyCall } from '../../lib/gctf-call-import';
import { joinEndpoint } from '../../lib/http-endpoint';
import { Upload, Terminal, AlertCircle, Check } from 'lucide-react';
import { useToast } from 'luvo/ui/ToastContext';

export function ImportPanel({ onDone }: { onDone?: () => void } = {}) {
  const toast = useToast();
  const [command, setCommand] = useState('');
  const intent = useStore(s => s.importIntent);
  useEffect(() => {
    if (intent > 0) setCommand(useStore.getState().importPrefill ?? '');
  }, [intent]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [summary, setSummary] = useState('');
  const [ignored, setIgnored] = useState<string[]>([]);
  const [adjusted, setAdjusted] = useState<string[]>([]);
  const setEndpoint = useStore(s => s.setEndpoint);
  const setRequestBodies = useStore(s => s.setRequestBodies);
  const setRequestHeaders = useStore(s => s.setRequestHeaders);
  const setAddress = useStore(s => s.setAddress);
  const setTls = useStore(s => s.setTls);
  const newWorkspace = useStore(s => s.newWorkspace);
  const focusHeldCall = useStore(s => s.focusHeldCall);
  const setSectionKv = useStore(s => s.setSectionKv);
  const setProtocol = useStore(s => s.setProtocol);
  const loadCollection = useStore(s => s.loadCollection);

  const importCurl = () => {
    const imported = parseCurl(parseShell(command.trim()));
    if (!imported.path) {
      setError('That curl command names no url');
      return;
    }
    const endpoint = joinEndpoint(imported.method, imported.path);
    if (focusHeldCall({ endpoint, headers: imported.headers, bodies: imported.body ? [imported.body] : [] })) {
      toast.success('Already open — this is the tab holding it');
      onDone?.();
      return;
    }
    newWorkspace();
    setEndpoint(endpoint);
    if (imported.address) setAddress(imported.address);
    setRequestBodies(imported.body ? [imported.body] : []);
    if (Object.keys(imported.headers).length > 0) setRequestHeaders(imported.headers);
    const said = curlSummary(imported).join(' · ');
    setSummary(said);
    setIgnored(imported.ignored);
    setAdjusted([]);
    setSuccess(true);
    settle(said, imported.ignored, []);
  };

  const importGrpctestify = async () => {
    const args = parseShell(command.trim());
    const sub = grpctestifySubcommand(args);
    if (sub !== 'call') {
      setError(sub === ''
        ? 'That line names no grpctestify subcommand — `call` is the one that is a single request'
        : `\`grpctestify ${sub}\` runs files rather than making one call — open the file from Collections`);
      return;
    }
    const imported = parseGrpctestifyCall(args);
    if (imported.endpoint === '' && imported.file !== '') {
      const opened = await loadCollection(imported.file, { pin: true });
      if (!opened) {
        setError(`That line runs ${imported.file}, and this project has no such file`);
        return;
      }
      toast.success(`Opened ${imported.file}`);
      onDone?.();
      return;
    }
    if (imported.endpoint === '') {
      setError('That line names no endpoint — `-e package.Service/Method`, or the file it runs');
      return;
    }
    if (focusHeldCall({
      endpoint: imported.endpoint,
      headers: imported.headers,
      bodies: imported.body ? [imported.body] : [],
    })) {
      toast.success('Already open — this is the tab holding it');
      onDone?.();
      return;
    }
    newWorkspace();
    setEndpoint(imported.endpoint);
    if (imported.address) setAddress(imported.address);
    setRequestBodies(imported.body ? [imported.body] : []);
    if (Object.keys(imported.headers).length > 0) setRequestHeaders(imported.headers);
    if (imported.protocol) setProtocol(imported.protocol);
    setTls(!imported.plaintext && (imported.insecure || Object.keys(imported.tls ?? {}).length > 0));
    const plan = planImport(imported);
    if (Object.keys(imported.tls ?? {}).length > 0) setSectionKv('tls', imported.tls!);
    if (Object.keys(plan.options).length > 0) setSectionKv('options', plan.options);
    const dropped = [...plan.ignored, ...imported.ignored];
    const said = [...callSummary(imported), ...(Object.keys(plan.options).length > 0 ? ['OPTIONS'] : [])];
    const sentence = said.length > 0 ? `with ${said.join(', ')}` : '';
    setSummary(sentence);
    setIgnored(dropped);
    setAdjusted(plan.adjusted);
    setSuccess(true);
    settle(sentence, dropped, plan.adjusted);
  };

  const settle = (said: string, dropped: string[], changed: string[]) => {
    if (dropped.length === 0 && changed.length === 0) {
      toast.success(`Imported ${said}`);
      onDone?.();
    }
  };

  const handleImport = async () => {
    if (!command.trim()) return;
    setLoading(true);
    setError(null);
    setSuccess(false);

    try {
      if (isCurl(command)) {
        importCurl();
        return;
      }
      if (isGrpctestify(command)) {
        await importGrpctestify();
        return;
      }
      const args = parseShell(command.trim());
      const res = await fetch('/api/import-grpcurl', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ args }),
      });

      let data: any = null;
      try { data = await res.json(); } catch {  }

      if (!res.ok) {
        setError(data?.error || `Import failed (${res.status})`);
        return;
      }
      if (data?.error) {
        setError(data.error);
        return;
      }

      if (focusHeldCall({
        endpoint: data.endpoint,
        headers: data.headers && Object.keys(data.headers).length > 0 ? data.headers : {},
        bodies: data.body ? [data.body] : [],
      })) {
        toast.success('Already open — this is the tab holding it');
        onDone?.();
        return;
      }
      newWorkspace();
      setEndpoint(data.endpoint);
      if (data.body) setRequestBodies([data.body]);
      if (data.address) setAddress(data.address);
      setTls(!data.plaintext);
      if (data.headers && Object.keys(data.headers).length > 0) {
        setRequestHeaders(data.headers);
      }

      const plan = planImport(data);
      if (data.tls && Object.keys(data.tls).length > 0) setSectionKv('tls', data.tls);
      if (data.proto && Object.keys(data.proto).length > 0) setSectionKv('proto', data.proto);
      if (Object.keys(plan.options).length > 0) setSectionKv('options', plan.options);

      const said = importSummary(data, plan);
      setSummary(said);
      setIgnored(plan.ignored);
      setAdjusted(plan.adjusted);
      setSuccess(true);
      settle(said, plan.ignored, plan.adjusted);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  const examples = [
    'grpcurl -plaintext localhost:4770 helloworld.Greeter/SayHello',
    "grpcurl -plaintext -d '{\"name\":\"World\"}' localhost:4770 helloworld.Greeter/SayHello",
    "curl https://api.example.com/v1/users",
    "curl -X POST https://api.example.com/v1/users -H 'content-type: application/json' -d '{\"name\":\"Ada\"}'",
    "grpctestify call -e 'helloworld.Greeter/SayHello' --address 'localhost:4770' -d '{\"name\":\"World\"}' --plaintext",
  ];

  return (
    <div className="stack">
      <div className="bar">
        <Upload size={14} className="muted" />
        <span className="label">Import a command</span>
      </div>

      <div className="muted">
        Paste a <span className="mono">grpcurl</span>, <span className="mono">curl</span> or
        <span className="mono"> grpctestify call</span> command to fill the request fields — the
        first word says which.
      </div>

      <fieldset className={`panel${error ? ' is-bad' : ''}`}>
        <legend>
          <Terminal size={11} />{' '}
          {command.trim() === ''
            ? 'command'
            : isCurl(command) ? 'curl command'
            : isGrpctestify(command) ? 'grpctestify command'
            : 'grpcurl command'}
        </legend>
        <textarea
          className="field mono paste-area"
          value={command}
          onChange={e => { setCommand(e.target.value); setError(null); setSuccess(false); setIgnored([]); }}
          placeholder="grpcurl -plaintext localhost:4770 package.Service/Method — or curl https://api.example.com/v1/users"
          rows={4}
          spellCheck={false}
        />
      </fieldset>

      {error && (
        <div className="assert is-fail">
          <span className="assert-mark"><AlertCircle size={12} /></span>
          <span>{error}</span>
        </div>
      )}

      {success && (
        <div className="stack is-tight">
          <div className="assert is-ok">
            <span className="assert-mark"><Check size={12} /></span>
            <span>Imported {summary}</span>
          </div>
          {ignored.length > 0 && (
            <div className="note is-warn">
              Not brought across: <span className="mono">{ignored.join(' · ')}</span> — nothing here
              holds them, so the call runs without them.
            </div>
          )}
          {adjusted.length > 0 && (
            <div className="note">
              Carried differently: <span className="mono">{adjusted.join(' · ')}</span>.
            </div>
          )}
          {onDone && (
            <div className="bar">
              <span className="grow" />
              <button className="btn is-primary" onClick={() => onDone()}>done</button>
            </div>
          )}
        </div>
      )}

      <button
        className="btn is-primary"
        onClick={handleImport}
        disabled={loading || !command.trim()}
      >
        {loading ? 'Parsing…' : 'Import'}
      </button>

      <div>
        <div className="label">Examples</div>
        {examples.map((ex, i) => (
          <button
            key={i}
            className="row example"
            onClick={() => { setCommand(ex); setError(null); setSuccess(false); }}
          >
            <span className="mono">{ex}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
