import { useState } from 'react';
import { ACTIVITY_OUTCOMES, type ActivityOutcome, type ActivityQuery } from '../../api/types';
import { useResource } from '../context';
import { Empty, Loaded, Page, SelectField } from '../ui';
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

export function ActivityView() {
  const [filters, setFilters] = useState<Filters>(EMPTY);
  // cursors[i] is the beforeId that produced page i; page 0 has none.
  const [cursors, setCursors] = useState<(number | undefined)[]>([undefined]);
  const page = cursors.length - 1;
  const hosts = useResource(api => api.listHosts(), []);
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
    <Page title="Activity" eyebrow="Every agent and owner action">
      <form className="toolbar filters" aria-label="Activity filters" onSubmit={e => e.preventDefault()}>
        <SelectField label="Outcome" value={filters.outcome} onChange={v => update({ outcome: v as ActivityOutcome | '' })}>
          <option value="">Any</option>
          {ACTIVITY_OUTCOMES.map(o => <option key={o} value={o}>{o}</option>)}
        </SelectField>
        <SelectField label="Tool" value={filters.tool} onChange={v => update({ tool: v })}>
          <option value="">Any</option>
          {KNOWN_TOOLS.map(t => <option key={t} value={t}>{t}</option>)}
        </SelectField>
        <SelectField label="Host" value={filters.hostId} onChange={v => update({ hostId: v })} disabled={hosts.status !== 'ready'}>
          <option value="">Any</option>
          {hosts.status === 'ready' ? hosts.data.map(h => <option key={h.id} value={h.id}>{h.name}</option>) : null}
        </SelectField>
        <label className="check">
          <input type="checkbox" checked={filters.includeReads} onChange={e => update({ includeReads: e.target.checked })} />
          <span>Include reads</span>
        </label>
        {filtered ? <button type="button" className="button button-quiet" onClick={() => update(EMPTY)}>Clear filters</button> : null}
      </form>
      <Loaded resource={activity} label="activity">
        {data => (
          <>
            {data.entries.length === 0
              ? <Empty title={filtered ? 'No activity matches these filters' : 'No activity yet'}>{filtered ? 'Clear a filter to see more.' : 'Agent and owner actions appear here as they happen.'}</Empty>
              : <ActivityList entries={data.entries} label={`Activity, page ${page + 1}`} />}
            <nav className="pager" aria-label="Activity pages">
              <button type="button" className="button" disabled={page === 0} onClick={() => setCursors(c => c.slice(0, -1))}>Newer</button>
              <span aria-live="polite">Page {page + 1}</span>
              <button type="button" className="button" disabled={data.nextBeforeId === null} onClick={() => setCursors(c => [...c, data.nextBeforeId ?? undefined])}>Older</button>
            </nav>
          </>
        )}
      </Loaded>
    </Page>
  );
}
