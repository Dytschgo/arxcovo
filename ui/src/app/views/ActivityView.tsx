import { useState, type CSSProperties } from 'react';
import { ACTIVITY_OUTCOMES, type ActivityEntry, type ActivityOutcome, type ActivityQuery } from '../../api/types';
import { useResource } from '../context';
import { outcomeCounts } from '../derive';
import { Empty, Loaded, OUTCOME_STATUS, OutcomeStatus, Page, SelectField, useNow } from '../ui';
import { Icon } from '../icons';
import { ActivityList } from './ActivityList';

export const PAGE_SIZE = 20;
export const KNOWN_TOOLS = ['run_command', 'discover_tools', 'list_hosts', 'list_activity', 'vm_create', 'vm_stop', 'snapshot_create', 'host_heartbeat'];

interface Filters {
  outcome: ActivityOutcome | '';
  tool: string;
  hostId: string;
  includeReads: boolean;
}

const EMPTY: Filters = { outcome: '', tool: '', hostId: '', includeReads: false };

/** Outcomes in the loaded rows only (the API has no totals): a split bar plus a legend with counts. */
function Outcomes({ entries }: { entries: ActivityEntry[] }) {
  const counts = outcomeCounts(entries);
  return (
    <section className="tile outcomes" aria-labelledby="outcomes-title">
      <p className="outcomes-n"><b>{entries.length}</b><span>rows loaded</span></p>
      <div className="outcomes-body">
        <header className="tile-head">
          <h2 id="outcomes-title">Outcomes</h2>
          <span className="tile-aside">in the {entries.length} loaded rows, not the whole log</span>
        </header>
        <div className="split" aria-hidden="true">
          {counts.filter(c => c.count > 0).map(c => <span key={c.outcome} data-outcome={c.outcome} style={{ flexGrow: c.count } as CSSProperties} />)}
        </div>
        <ul className="split-legend">
          {counts.map(c => (
            <li key={c.outcome} data-outcome={c.outcome}>
              <span className="sw" data-outcome={c.outcome} aria-hidden="true" />
              <OutcomeStatus outcome={c.outcome} />
              <b>{c.count}</b>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}

export function ActivityView() {
  const [filters, setFilters] = useState<Filters>(EMPTY);
  // cursors[i] is the beforeId that produced page i; page 0 has none.
  const [cursors, setCursors] = useState<(number | undefined)[]>([undefined]);
  const page = cursors.length - 1;
  const hosts = useResource(api => api.listHosts(), []);
  const now = useNow(30_000);
  const query: ActivityQuery = {
    limit: PAGE_SIZE,
    beforeId: cursors[page],
    outcome: filters.outcome || undefined,
    tool: filters.tool || undefined,
    hostId: filters.hostId || undefined,
    includeReads: filters.includeReads,
  };
  const activity = useResource(api => api.listActivity(query), [JSON.stringify(query)]);

  function update(patch: Partial<Filters>) {
    setFilters(f => ({ ...f, ...patch }));
    setCursors([undefined]);
  }

  const filtered = filters.outcome !== '' || filters.tool !== '' || filters.hostId !== '';

  return (
    <Page title="Activity" lede="Every agent, owner and system action, newest first.">
      {activity.status === 'ready' && activity.data.entries.length > 0 ? <Outcomes entries={activity.data.entries} /> : null}
      <form className="toolbar filters" aria-label="Activity filters" onSubmit={e => e.preventDefault()}>
        <div className="chips-group" role="group" aria-label="Outcome">
          <button type="button" className="fchip" aria-pressed={filters.outcome === ''} onClick={() => update({ outcome: '' })}>All</button>
          {ACTIVITY_OUTCOMES.map(o => (
            <button key={o} type="button" className="fchip" aria-pressed={filters.outcome === o} onClick={() => update({ outcome: o })} title={`Only ${OUTCOME_STATUS[o].word}`}>
              <OutcomeStatus outcome={o} />
            </button>
          ))}
        </div>
        <SelectField label="Tool" value={filters.tool} onChange={v => update({ tool: v })}>
          <option value="">All tools</option>
          {KNOWN_TOOLS.map(t => <option key={t} value={t}>{t}</option>)}
        </SelectField>
        <SelectField label="Host" value={filters.hostId} onChange={v => update({ hostId: v })} disabled={hosts.status !== 'ready'}>
          <option value="">All hosts</option>
          {hosts.status === 'ready' ? hosts.data.map(h => <option key={h.id} value={h.id}>{h.name}</option>) : null}
        </SelectField>
        <label className="switch">
          <input type="checkbox" role="switch" checked={filters.includeReads} onChange={e => update({ includeReads: e.target.checked })} />
          <span className="switch-track" aria-hidden="true" />
          <span>Include reads</span>
        </label>
        {filtered ? <button type="button" className="button button-quiet" onClick={() => update(EMPTY)}>Clear filters</button> : null}
      </form>
      <Loaded resource={activity} label="activity">
        {data => (
          <div className="tile activity-tile">
            {data.entries.length === 0
              ? <Empty title={filtered ? 'No activity matches these filters' : 'No activity yet'}>{filtered ? 'Clear a filter to see more.' : 'Agent and owner actions appear here as they happen.'}</Empty>
              : (
                <>
                  <div className="act-head" aria-hidden="true">
                    <span>Time</span><span>Tool</span><span>Summary</span><span>Actor</span><span>Host / VM</span><span>Exit</span><span>Outcome</span>
                  </div>
                  <ActivityList entries={data.entries} label={`Activity, page ${page + 1}`} now={now} />
                </>
              )}
            <nav className="pager" aria-label="Activity pages">
              <button type="button" className="button" disabled={page === 0} onClick={() => setCursors(c => c.slice(0, -1))}><Icon name="back" />Newer</button>
              <span aria-live="polite">Page {page + 1} · {data.entries.length} rows</span>
              <button type="button" className="button" disabled={data.nextBeforeId === null} onClick={() => setCursors(c => [...c, data.nextBeforeId ?? undefined])}>Older<Icon name="next" /></button>
            </nav>
          </div>
        )}
      </Loaded>
      <p className="foot-note">Pages of {PAGE_SIZE}, newest first. Summaries are redacted by the server.</p>
    </Page>
  );
}
