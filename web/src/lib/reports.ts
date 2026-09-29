export type ReportFormat = 'json' | 'junit' | 'yaml' | 'html' | 'allure';

export function isDirectoryReport(file: string): boolean {
  return file === 'allure';
}

export function reportFileName(format: ReportFormat): string {
  if (format === 'junit') return 'junit.xml';
  if (format === 'allure') return 'allure';
  return `report.${format}`;
}

export function reportsDirOf(projectRoot: string | null): string {
  if (!projectRoot) return 'grpctestify-reports';
  return `${projectRoot.replace(/^\.\//, '').replace(/\/+$/, '')}/reports`;
}

export const KEEP_RUNS = 20;

export function reportsPathOf(projectRoot: string | null, jobId: string): string {
  return `${reportsDirOf(projectRoot)}/${jobId}`;
}

export function reportRefusal(file: string, status: number): string {
  if (status === 404) {
    return `${file} is not on the server any more — the last ${KEEP_RUNS} runs are kept`;
  }
  return `${file} could not be read (${status})`;
}

export const REPORT_FORMATS: ReportFormat[] = ['json', 'junit', 'yaml', 'html', 'allure'];

export function allureNote(
  written: { files?: number; path: string; open: string },
  copied: boolean,
): string {
  const many = written.files === 1 ? '1 result' : `${written.files ?? 0} results`;
  const where = `${many} in ${written.path}`;
  return copied
    ? `${where} — \`${written.open}\` copied, and it reads that path from the project directory`
    : `${where} — the browser refused the clipboard, so run \`${written.open}\` from the project directory`;
}

export function downloadableReports(written: string[]): { file: string; ready: boolean }[] {
  const already = new Set(written);
  const rows = REPORT_FORMATS.map(format => {
    const file = reportFileName(format);
    return { file, ready: already.has(file) };
  });
  for (const file of written) {
    if (!rows.some(row => row.file === file)) rows.push({ file, ready: true });
  }
  return rows;
}

export function writeNote(format: ReportFormat, writing: boolean, reportsDir: string): string {
  const verb = writing ? 'Stop writing' : 'Write';
  const when = writing ? '' : ' when a run ends';
  const what = reportFileName(format);
  const kept = `the last ${KEEP_RUNS} kept`;
  return isDirectoryReport(what)
    ? `${verb} a directory of Allure results into ${reportsDir}/<run>/${what}${when} — a directory per run, ${kept}, and the run hands over its path for \`allure serve\``
    : `${verb} ${what} into ${reportsDir}/<run>${when} — a directory per run, ${kept}, and the run offers it to download`;
}
