export type Inline =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'strong'; text: string }
  | { kind: 'link'; text: string; href: string };

export type Block =
  | { kind: 'heading'; level: number; text: Inline[] }
  | { kind: 'para'; text: Inline[] }
  | { kind: 'code'; lang: string; text: string }
  | { kind: 'table'; head: Inline[][]; rows: Inline[][][] }
  | { kind: 'list'; items: Inline[][] }
  | { kind: 'rule' };

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let plain = '';
  const flush = () => { if (plain !== '') { out.push({ kind: 'text', text: plain }); plain = ''; } };

  for (let i = 0; i < text.length; i++) {
    const rest = text.slice(i);

    if (text[i] === '`') {
      const fence = /^`+/.exec(rest)![0];
      const close = rest.indexOf(fence, fence.length);
      if (close > 0 && !rest.slice(fence.length, close).includes('\n')) {
        flush();
        out.push({ kind: 'code', text: rest.slice(fence.length, close).replace(/^ (.*) $/s, '$1') });
        i += close + fence.length - 1;
        continue;
      }
    }

    const bold = /^\*\*([^*]+)\*\*/.exec(rest);
    if (bold) {
      flush();
      out.push({ kind: 'strong', text: bold[1]! });
      i += bold[0].length - 1;
      continue;
    }

    const link = /^\[([^\]]+)\]\(([^)]+)\)/.exec(rest);
    if (link) {
      flush();
      out.push({ kind: 'link', text: link[1]!, href: link[2]! });
      i += link[0].length - 1;
      continue;
    }

    plain += text[i];
  }
  flush();
  return out;
}

const cells = (line: string): Inline[][] => {
  const out: string[] = [];
  let current = '';
  const body = line.replace(/^\|/, '').replace(/\|$/, '');
  for (let i = 0; i < body.length; i++) {
    const ch = body[i]!;
    if (ch === '\\' && (body[i + 1] === '|' || body[i + 1] === '\\')) {
      current += body[++i];
      continue;
    }
    if (ch === '|') { out.push(current); current = ''; continue; }
    current += ch;
  }
  out.push(current);
  return out.map(c => parseInline(c.trim()));
};

export function parseMarkdown(md: string): Block[] {
  const lines = md.split('\n');
  const blocks: Block[] = [];
  let para: string[] = [];

  const flush = () => {
    if (para.length > 0) {
      blocks.push({ kind: 'para', text: parseInline(para.join(' ')) });
      para = [];
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    if (/^[-*] /.test(line)) {
      flush();
      const items: Inline[][] = [];
      while (i < lines.length && /^[-*] /.test(lines[i]!)) {
        items.push(parseInline(lines[i]!.slice(2).trim()));
        i++;
      }
      i--;
      blocks.push({ kind: 'list', items });
      continue;
    }

    if (line.startsWith('```')) {
      flush();
      const lang = line.slice(3).trim();
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].startsWith('```')) {
        body.push(lines[i]);
        i += 1;
      }
      blocks.push({ kind: 'code', lang, text: body.join('\n') });
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      flush();
      blocks.push({ kind: 'heading', level: heading[1].length, text: parseInline(heading[2]) });
      continue;
    }

    if (/^\s*---+\s*$/.test(line)) {
      flush();
      blocks.push({ kind: 'rule' });
      continue;
    }

    if (line.trim().startsWith('|') && /^\s*\|[\s:|-]+\|\s*$/.test(lines[i + 1] ?? '')) {
      flush();
      const head = cells(line.trim());
      const rows: Inline[][][] = [];
      i += 2;
      while (i < lines.length && lines[i].trim().startsWith('|')) {
        rows.push(cells(lines[i].trim()));
        i += 1;
      }
      i -= 1;
      blocks.push({ kind: 'table', head, rows });
      continue;
    }

    if (line.trim() === '') flush();
    else para.push(line.trim());
  }

  flush();
  return blocks;
}
