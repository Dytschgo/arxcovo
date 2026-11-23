import type { Host } from '../../api/types';
import { useResource } from '../context';
import { OS_LABEL, formatBytes, formatDate, percent } from '../format';
import { href } from '../router';
import { ArchBadge, Empty, Loaded, Meter, Page, StatePill } from '../ui';

export function HostMeters({ host }: { host: Host }) {
  const diskUsed = host.disk.totalBytes - host.disk.freeBytes;
  return (
    <div className="meters">
      <Meter label="CPU" value={host.cpu.usedPercent === null ? null : Math.round(host.cpu.usedPercent)} detail={`${host.cpu.cores} cores`} />
      <Meter label="Memory" value={percent(host.memory.usedBytes, host.memory.totalBytes)} detail={host.memory.usedBytes === null ? `${formatBytes(host.memory.totalBytes)} total` : `${formatBytes(host.memory.usedBytes)} of ${formatBytes(host.memory.totalBytes)}`} />
      <Meter label="Disk" value={percent(diskUsed, host.disk.totalBytes)} detail={`${formatBytes(host.disk.freeBytes)} free of ${formatBytes(host.disk.totalBytes)}`} />
    </div>
  );
}

function HostCard({ host }: { host: Host }) {
  return (
    <li className="card host-card" data-reachability={host.reachability}>
      <div className="card-head">
        <h2 className="card-title">
          <a className="card-link" href={href({ name: 'host', id: host.id })}>{host.name}</a>
        </h2>
        <StatePill value={host.reachability} />
      </div>
      <p className="card-sub">
        <span>{OS_LABEL[host.os]} · {host.osVersion}</span>
        <ArchBadge arch={host.arch} />
      </p>
      {host.reachabilityReason ? <p className="card-reason">{host.reachabilityReason}</p> : null}
      <HostMeters host={host} />
      <p className="card-foot">
        <span>{host.tools.length} tools</span>
        <span>{host.mode === 'always_on' ? 'Always on' : 'Opportunistic'}</span>
        <span>Seen {formatDate(host.lastSeenAt)}</span>
      </p>
    </li>
  );
}

export function HostsView() {
  const hosts = useResource(api => api.listHosts(), []);
  return (
    <Page title="Hosts" eyebrow="Cluster">
      <Loaded resource={hosts} label="hosts">
        {list => list.length === 0
          ? <Empty title="No hosts yet">Install arxd on a computer and join it with a one-time join code.</Empty>
          : (
            <>
              <p className="summary" aria-live="polite">
                {list.filter(h => h.reachability === 'online').length} of {list.length} hosts online
              </p>
              <ul className="card-grid" aria-label="Hosts">
                {list.map(h => <HostCard key={h.id} host={h} />)}
              </ul>
            </>
          )}
      </Loaded>
    </Page>
  );
}
