import { useState, useEffect } from 'react';
import { btn, colors } from '../../lib/theme';
import { Upload, FileText, Check, AlertCircle, Loader2 } from 'lucide-react';

interface ProtoFile {
  path: string;
  name: string;
  size: number;
}

export function ProtoManager() {
  const [files, setFiles] = useState<ProtoFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [uploadName, setUploadName] = useState('');
  const [uploadContent, setUploadContent] = useState('');
  const [uploading, setUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [uploadSuccess, setUploadSuccess] = useState(false);

  const fetchFiles = async () => {
    setLoading(true);
    try {
      const res = await fetch('/api/proto-files');
      if (res.ok) setFiles(await res.json());
    } catch {  }
    setLoading(false);
  };

  useEffect(() => { fetchFiles(); }, []);

  const handleUpload = async () => {
    if (!uploadName.trim() || !uploadContent.trim()) return;
    setUploading(true);
    setUploadError(null);
    setUploadSuccess(false);
    try {
      const res = await fetch('/api/proto-upload', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ filename: uploadName.trim(), content: uploadContent }),
      });
      if (res.ok) {
        setUploadSuccess(true);
        setUploadName('');
        setUploadContent('');
        fetchFiles();
        setTimeout(() => setUploadSuccess(false), 3000);
      } else {
        const text = await res.text();
        setUploadError(text || 'Upload failed');
      }
    } catch (err) {
      setUploadError(String(err));
    } finally {
      setUploading(false);
    }
  };

  const exampleProto = `syntax = "proto3";

service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
}

message HelloRequest {
  string name = 1;
}

message HelloReply {
  string message = 1;
}
`;

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
        <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.6px' }}>
          Proto Files
        </span>
        <span style={{ flex: 1 }} />
        {loading && <Loader2 size={12} className="animate-spin" />}
      </div>

      {}
      <div style={{ border: '1px solid var(--border)', borderRadius: 6, padding: 8, marginBottom: 8 }}>
        <input value={uploadName} onChange={e => setUploadName(e.target.value)} placeholder="filename.proto"
          style={{ width: '100%', border: '1px solid var(--border)', borderRadius: 4, padding: '4px 8px', fontSize: 12, fontFamily: 'monospace', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none', boxSizing: 'border-box', marginBottom: 4 }} />
        <textarea value={uploadContent} onChange={e => setUploadContent(e.target.value)} placeholder={`Paste .proto content here...\n\n${exampleProto}`} rows={4}
          style={{ width: '100%', border: '1px solid var(--border)', borderRadius: 4, padding: '4px 8px', fontSize: 11, fontFamily: 'monospace', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none', resize: 'vertical', boxSizing: 'border-box', marginBottom: 4 }} />
        <button onClick={handleUpload} disabled={uploading || !uploadName.trim() || !uploadContent.trim()}
          style={{ ...btn('primary'), width: '100%', opacity: uploading ? 0.5 : 1, cursor: uploading ? 'not-allowed' : 'pointer', fontSize: 12 }}>
          {uploading ? <Loader2 size={12} className="animate-spin" /> : <Upload size={12} />}
          {uploading ? 'Uploading…' : 'Upload .proto'}
        </button>

        {uploadError && <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 11, color: colors.error, marginTop: 4 }}><AlertCircle size={11} /> {uploadError}</div>}
        {uploadSuccess && <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 11, color: colors.success, marginTop: 4 }}><Check size={11} /> Uploaded</div>}
      </div>

      {}
      {files.length === 0 && (
        <div style={{ fontSize: 12, color: 'var(--text-muted)', padding: '8px 0', textAlign: 'center' }}>
          No .proto files uploaded yet.
        </div>
      )}

      {files.map(f => (
        <div key={f.path} style={{
          display: 'flex', alignItems: 'center', gap: 6, padding: '4px 6px', borderRadius: 4,
          marginBottom: 2, fontSize: 12,
        }}
          onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
          onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; }}
        >
          <FileText size={13} color={colors.accent} />
          <span style={{ fontFamily: 'monospace', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{f.name}</span>
          <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>{(f.size / 1024).toFixed(1)}KB</span>
        </div>
      ))}
    </div>
  );
}
