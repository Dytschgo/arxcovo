import type { ActivityEntry } from '../../api/types';
import { formatDate } from '../format';
import { href } from '../router';
import { Pill, StatePill } from '../ui';

export function ActivityList({ entries, label }: { entries: ActivityEntry[]; label: string }) {
  return (
    <ol className="activity" aria-label={label}>
      {entries.map(e => (
        <li key={e.id} className="activity-item" data-outcome={e.outcome}>
          <div className="activity-main">
            <code className="activity-tool">{e.tool}</code>
            <span className="activity-summary">{e.summary}</span>
          </div>
          <div className="activity-meta">
            <Pill tone={e.actor.kind === 'agent' ? 'info' : 'muted'}>
              <span className="visually-hidden">{e.actor.kind} </span>{e.actor.name}
            </Pill>
            {e.hostId ? <a href={href({ name: 'host', id: e.hostId })}>{e.hostId}</a> : null}
            {e.vmId ? <a href={href({ name: 'vm', id: e.vmId })}>{e.vmId}</a> : null}
            <time dateTime={e.startedAt ?? undefined}>{formatDate(e.startedAt)}</time>
            {e.exitCode !== null ? <span>exit {e.exitCode}</span> : null}
            <StatePill value={e.outcome} />
          </div>
        </li>
      ))}
    </ol>
  );
}
