import { describe, it, expect } from 'vitest';
import { KEEP_RUNS, downloadableReports, isDirectoryReport, writeNote, reportFileName, reportRefusal, reportsDirOf, reportsPathOf } from './reports';

describe('report names and places', () => {
  it('names the file each writer writes', () => {
    expect(reportFileName('junit')).toBe('junit.xml');
    expect(reportFileName('json')).toBe('report.json');
  });

  it('puts a run in a directory of its own', () => {
    expect(reportsPathOf('./.grpctestify', 'abc')).toBe('.grpctestify/reports/abc');
    expect(reportsPathOf(null, 'abc')).toBe('grpctestify-reports/abc');
  });

  it('says why a download did not happen', () => {
    expect(reportRefusal('report.json', 404)).toContain(`last ${KEEP_RUNS} runs`);
    expect(reportRefusal('report.json', 500)).toBe('report.json could not be read (500)');
  });

  it('reads the same directory the writers use', () => {
    expect(reportsDirOf('.grpctestify')).toBe('.grpctestify/reports');
  });
});

describe('the page a report is shared as', () => {
  it('is named the way the writer names it', () => {
    expect(reportFileName('html')).toBe('report.html');
  });
});

describe('downloadableReports', () => {
  it('offers every format, and says which are already on disk', () => {
    expect(downloadableReports(['report.json'])).toEqual([
      { file: 'report.json', ready: true },
      { file: 'junit.xml', ready: false },
      { file: 'report.yaml', ready: false },
      { file: 'report.html', ready: false },
      { file: 'allure', ready: false },
    ]);
  });

  it('offers them all when the run wrote none', () => {
    expect(downloadableReports([]).every(r => !r.ready)).toBe(true);
    expect(downloadableReports([])).toHaveLength(5);
  });

  it('keeps a file it does not name', () => {
    expect(downloadableReports(['allure/'])).toContainEqual({ file: 'allure/', ready: true });
  });
});

describe('the report that is a directory', () => {
  it('is offered beside the files', () => {
    expect(downloadableReports([]).map(r => r.file)).toContain('allure');
  });

  it('is named the way the run writes it', () => {
    expect(reportFileName('allure')).toBe('allure');
  });

  it('is the only one that is not a download', () => {
    expect(isDirectoryReport('allure')).toBe(true);
    for (const file of ['report.json', 'junit.xml', 'report.yaml', 'report.html']) {
      expect(isDirectoryReport(file)).toBe(false);
    }
  });

  it('reads as written once the run has written it', () => {
    const row = downloadableReports(['allure']).find(r => r.file === 'allure');
    expect(row?.ready).toBe(true);
  });
});

describe('what ticking a format promises', () => {
  it('offers a download for the formats that are one file', () => {
    const said = writeNote('junit', false, '.grpctestify/reports');
    expect(said).toContain('Write junit.xml into .grpctestify/reports/<run> when a run ends');
    expect(said).toContain('offers it to download');
  });

  it('promises a path, not a download, for the one that is a directory', () => {
    const said = writeNote('allure', false, '.grpctestify/reports');
    expect(said).toContain('a directory of Allure results into .grpctestify/reports/<run>/allure');
    expect(said).not.toContain('download');
    expect(said).toContain('allure serve');
  });

  it('says how to stop once it is on', () => {
    expect(writeNote('json', true, 'reports')).toContain('Stop writing report.json');
    expect(writeNote('json', true, 'reports')).not.toContain('when a run ends');
  });

  it('names how many runs are kept', () => {
    expect(writeNote('yaml', false, 'reports')).toContain(`the last ${KEEP_RUNS} kept`);
  });
});
