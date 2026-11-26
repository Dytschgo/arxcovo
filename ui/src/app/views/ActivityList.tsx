import type { ActivityEntry, ActorKind } from '../../api/types';
import { formatAgo, formatDate } from '../format';
import { Icon, type IconName } from '../icons';
import { href } from '../router';
import { NullValue, OutcomeStatus } from '../ui';

const ACTOR_ICON: Record<ActorKind, IconName> = { agent: 'bot', owner: 'user', system: 'cog' };

/** Actor with its kind word always shown: "agent · builder", "owner", "system · arxd". */
export function Actor({ actor }: { actor: ActivityEntry['actor'] }) {
  return (
    <span className="act-actor">
      <Icon name={ACTOR_ICON[actor.kind]} />
      <span className="act-kind">{actor.kind}</span>
      {actor.name !== actor.kind ? <><span aria-hidden="true">·</span><span className="act-name">{actor.name}</span></> : null}
    </span>
  );
}

/**
 * Activity rows: time, tool (mono), summary, actor, host / VM, exit code when
 * present, outcome. Kept as a list so each entry reads as one item; the grid
 * columns are visual only.
 */
export function ActivityList({ entries, label, now }: { entries: ActivityEntry[]; label: string; now: number }) {
  return (
    <ol className="activity" aria-label={label}>
      {entries.map(e => (
        <li key={e.id} className="activity-item" data-outcome={e.outcome}>
          <span className="act-time">
            {e.startedAt === null
              ? <NullValue reason="no time" />
              : <time dateTime={e.startedAt} title={formatDate(e.startedAt)}>{formatAgo(e.startedAt, now)}</time>}
          </span>
          <code className="act-tool">{e.tool}</code>
          <span className="act-sum">{e.summary !== e.tool ? e.summary : null}</span>
          <Actor actor={e.actor} />
          <span className="act-where">
            {e.hostId ? <a href={href({ name: 'host', id: e.hostId })}>{e.hostId}</a> : null}
            {e.hostId && e.vmId ? <span aria-hidden="true"> / </span> : null}
            {e.vmId ? <a href={href({ name: 'vm', id: e.vmId })}>{e.vmId}</a> : null}
          </span>
          <span className="act-exit">{e.exitCode !== null ? `exit ${e.exitCode}` : null}</span>
          <span className="act-out"><OutcomeStatus outcome={e.outcome} /></span>
        </li>
      ))}
    </ol>
  );
}
