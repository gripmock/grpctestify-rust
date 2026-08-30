import { useCallback, useEffect, useState } from 'react';
import { useStore } from '../../lib/store';
import { familyOf } from '../../lib/tree';
import { useToast } from 'luvo/ui/ToastContext';
import { bytesToBase64, protoKindOf, refusalFor } from '../../lib/proto-files';
import { summariseDrop, type DropOutcome } from '../../lib/drop-summary';

export function FileDrop() {
  const [over, setOver] = useState(false);
  const toast = useToast();
  const refreshCollections = useStore(s => s.refreshCollections);

  const take = useCallback(async (files: File[]) => {
    const outcomes: DropOutcome[] = [];
    for (const file of files) {
      const kind = protoKindOf(file.name);
      if (kind) {
        const body = kind === 'descriptor'
          ? {
              filename: file.name,
              encoding: 'base64',
              content: bytesToBase64(new Uint8Array(await file.arrayBuffer())),
            }
          : { filename: file.name, content: await file.text() };
        const res = await fetch('/api/proto-upload', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        if (res.ok) {
          outcomes.push({ kind: 'schema', name: file.name });
          refreshCollections();
        } else {
          outcomes.push({ kind: 'refused', name: file.name, reason: (await res.text()).trim() });
        }
        continue;
      }

      if (familyOf(file.name) !== 'unknown') {
        useStore.getState().openDroppedFile(file.name, await file.text());
        outcomes.push({ kind: 'opened', name: file.name });
        continue;
      }

      outcomes.push({ kind: 'refused', name: file.name, reason: refusalFor(file.name) });
    }

    const said = summariseDrop(outcomes, {
      fileOpen: useStore.getState().workspacePath !== null,
    });
    if (!said) return;
    if (said.failed) toast.error(said.text);
    else toast.success(said.text);
  }, [refreshCollections, toast]);

  useEffect(() => {
    const hasFiles = (e: DragEvent) => e.dataTransfer?.types?.includes('Files') ?? false;

    const over = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      setOver(true);
    };
    const leave = (e: DragEvent) => {
      if (e.relatedTarget === null) setOver(false);
    };
    const drop = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      setOver(false);
      void take([...(e.dataTransfer?.files ?? [])]);
    };

    window.addEventListener('dragover', over);
    window.addEventListener('dragleave', leave);
    window.addEventListener('drop', drop);
    return () => {
      window.removeEventListener('dragover', over);
      window.removeEventListener('dragleave', leave);
      window.removeEventListener('drop', drop);
    };
  }, [take]);

  if (!over) return null;

  return (
    <div className="file-drop">
      <div className="drop">
        <span>drop a .gctf, a .httf, a .proto or a descriptor set here</span>
        <span className="muted">a test file opens as a tab · a schema joins the collections</span>
      </div>
    </div>
  );
}
