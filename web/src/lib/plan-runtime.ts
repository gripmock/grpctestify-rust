export interface RuntimeOption {
  key: string;
  value: string;
  source: string;
}

export interface RuntimeRow {
  key: string;
  value: string;
  from: string;
  fromFile: boolean;
}

const CLI_DEFAULT = 'CLI default';

export function runtimeRow(option: RuntimeOption): RuntimeRow {
  return {
    key: option.key,
    value: option.value,
    from: decidedBy(option.source),
    fromFile: option.source !== CLI_DEFAULT,
  };
}

function decidedBy(source: string): string {
  if (source === CLI_DEFAULT) return 'default';
  if (source === 'OPTIONS') return 'set in OPTIONS';
  if (source === 'attribute') return 'set on the section';
  return source;
}

export interface TransportDrift {
  chosen: string;
  file: string;
}

export function transportDrift(
  rows: RuntimeRow[],
  workbench: string,
  pending?: string | null,
): TransportDrift | null {
  const row = rows.find(r => r.key === 'protocol');
  if (!row || row.fromFile) return null;
  if (!workbench || workbench === row.value) return null;
  if (pending && pending === workbench) return null;
  return { chosen: workbench, file: row.value };
}
