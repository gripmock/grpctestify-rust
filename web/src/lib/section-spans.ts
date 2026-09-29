export interface SectionSpan {
  section: string;
  start_line: number;
  end_line: number;
  content: string;
}

export interface SpanGroup {
  step: number;
  sections: SectionSpan[];
}

export function groupSectionsByStep(sections: SectionSpan[]): SpanGroup[] {
  const groups: SpanGroup[] = [];
  let current: SectionSpan[] = [];
  let seenEndpoint = false;

  for (const section of sections) {
    if (section.section === 'ENDPOINT' && seenEndpoint) {
      groups.push({ step: groups.length + 1, sections: current });
      current = [];
    }
    if (section.section === 'ENDPOINT') seenEndpoint = true;
    current.push(section);
  }
  if (current.length > 0) groups.push({ step: groups.length + 1, sections: current });
  return groups;
}

export function sectionLines(span: { start_line: number; end_line: number }): string {
  const first = span.start_line;
  const last = Math.max(span.end_line, first);
  return first === last ? `${first}` : `${first}–${last}`;
}

export interface RuntimeOption { key: string; value: string; source: string }

export function sameAsFirst(runtimes: RuntimeOption[][]): boolean[] {
  const first = runtimes[0];
  return runtimes.map((options, i) => {
    if (i === 0 || !first) return false;
    if (options.length !== first.length) return false;
    return options.every((o, j) => o.key === first[j].key && o.value === first[j].value && o.source === first[j].source);
  });
}
