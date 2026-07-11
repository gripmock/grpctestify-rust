import { useState } from 'react';
import { useStore } from '../../lib/store';
import { parseShell } from '../../lib/shell';
import { Upload, Terminal, AlertCircle, Check } from 'lucide-react';

export function ImportPanel() {
  const [command, setCommand] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const setEndpoint = useStore(s => s.setEndpoint);
  const setRequestBodies = useStore(s => s.setRequestBodies);
  const setRequestHeaders = useStore(s => s.setRequestHeaders);
  const setAddress = useStore(s => s.setAddress);
  const newWorkspace = useStore(s => s.newWorkspace);

  const handleImport = async () => {
    if (!command.trim()) return;
    setLoading(true);
    setError(null);
    setSuccess(false);

    try {
      const args = parseShell(command.trim());
      const res = await fetch('/api/import-grpcurl', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ args }),
      });
      const data = await res.json();

      if (data.error) {
        setError(data.error);
        return;
      }

      
      newWorkspace();
      setEndpoint(data.endpoint);
      if (data.body) setRequestBodies([data.body]);
      if (data.address) setAddress(data.address);
      if (data.headers && Object.keys(data.headers).length > 0) {
        setRequestHeaders(data.headers);
      }
      setSuccess(true);
      setTimeout(() => setSuccess(false), 3000);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  const examples = [
    'grpcurl -plaintext localhost:4770 helloworld.Greeter/SayHello',
    "grpcurl -plaintext -d '{\"name\":\"World\"}' localhost:4770 helloworld.Greeter/SayHello",
    "grpcurl -H 'x-api-key: abc123' -plaintext localhost:4770 svc.Service/Method",
  ];

  return (
    <div style={{ padding: 8 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
        <Upload size={14} />
        <span style={{
          fontSize: 11, fontWeight: 600, color: 'var(--text-secondary)',
          textTransform: 'uppercase', letterSpacing: '0.5px',
        }}>
          Import from grpcurl
        </span>
      </div>

      <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginBottom: 8, lineHeight: 1.5 }}>
        Paste a <code style={{ background: 'var(--bg-tertiary)', padding: '1px 4px', borderRadius: 3 }}>grpcurl</code> command to automatically fill the request fields.
      </div>

      <div style={{
        border: `1px solid ${error ? 'var(--error)' : 'var(--border)'}`,
        borderRadius: 6, overflow: 'hidden', marginBottom: 8,
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 4, padding: '4px 8px',
          background: 'var(--bg-tertiary)', fontSize: 11, color: 'var(--text-muted)',
        }}>
          <Terminal size={12} /> grpcurl command
        </div>
        <textarea
          value={command}
          onChange={e => { setCommand(e.target.value); setError(null); setSuccess(false); }}
          placeholder="grpcurl -plaintext localhost:4770 package.Service/Method"
          rows={4}
          style={{
            width: '100%', border: 'none', resize: 'vertical', padding: 8, fontSize: 12,
            fontFamily: 'monospace', background: 'var(--bg-primary)', color: 'var(--text-primary)',
            outline: 'none', boxSizing: 'border-box',
          }}
        />
      </div>

      {error && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 4, fontSize: 12,
          color: 'var(--error)', marginBottom: 8,
        }}>
          <AlertCircle size={12} /> {error}
        </div>
      )}

      {success && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 4, fontSize: 12,
          color: 'var(--success)', marginBottom: 8,
        }}>
          <Check size={12} /> Imported successfully
        </div>
      )}

      <button
        onClick={handleImport}
        disabled={loading || !command.trim()}
        style={{
          display: 'flex', alignItems: 'center', gap: 6, padding: '8px 16px', width: '100%',
          justifyContent: 'center', background: 'var(--accent)', color: '#fff', border: 'none',
          borderRadius: 6, cursor: loading || !command.trim() ? 'not-allowed' : 'pointer',
          fontSize: 13, fontWeight: 500, opacity: loading || !command.trim() ? 0.6 : 1,
        }}
      >
        {loading ? 'Parsing...' : 'Import'}
      </button>

      <div style={{ marginTop: 12 }}>
        <div style={{
          fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase',
          letterSpacing: '0.5px', marginBottom: 6,
        }}>
          Examples
        </div>
        {examples.map((ex, i) => (
          <div
            key={i}
            onClick={() => { setCommand(ex); setError(null); setSuccess(false); }}
            style={{
              padding: '6px 8px', fontSize: 11, fontFamily: 'monospace', borderRadius: 4,
              cursor: 'pointer', marginBottom: 2, background: 'var(--bg-tertiary)',
              color: 'var(--text-secondary)', wordBreak: 'break-all', lineHeight: 1.4,
            }}
            onMouseEnter={e => { (e.currentTarget as HTMLElement).style.background = 'var(--border)'; }}
            onMouseLeave={e => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-tertiary)'; }}
          >
            {ex}
          </div>
        ))}
      </div>
    </div>
  );
}
