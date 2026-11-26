import type { Host } from '../../api/types';
import { useInventory, useResource } from '../context';
import { allocation } from '../derive';
import { OS_LABEL, formatAgo, formatBytes, formatDate, percent } from '../format';
import { Icon } from '../icons';
import { href } from '../router';
import { ArchBadge, BackLink, CapBar, Empty, Facts, HostStatus, Loaded, NullValue, OsBadge, Tile, VmCard, useNow, usePageHeading } from '../ui';
import { ActivityList } from './ActivityList';

// Host detail as a Night Orbit bento (docs/design/night-orbit/host.html).
// CPU shows the current sample only: CPU history has no API yet, so there is
// no trace or time window. Remote desktop and the disk reserve are not in the
// API either and are not drawn.

/** A ring for a used/total pair. `role="meter"` only when there is a reading, so null never reads as 0. */
function Ring({ label, pct }: { label: string; pct: number | null }) {
  const art = (
    <svg className="ring-svg" viewBox="0 0 100 100" aria-hidden="true" focusable="false">
      <circle className="ring-track" data-meter-track="" cx="50" cy="50" r="42" pathLength={100} />
      {pct === null ? null : <circle className="ring-fill" data-meter-fill="" cx="50" cy="50" r="42" pathLength={100} strokeDasharray={`${pct} 100`} />}
    </svg>
  );
  return pct === null
    ? <div className="ring" data-unknown="true" aria-hidden="true">{art}</div>
    : <div className="ring" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-valuetext={`${pct}%`}>{art}</div>;
}

function CpuTile({ host }: { host: Host }) {
  const used = host.cpu.usedPercent === null ? null : Math.round(host.cpu.usedPercent);
  return (
    <Tile id="host-cpu" icon="cpu" title="CPU" className="s7 cpu-tile" aside={<span className="mono">{host.cpu.model} · {host.cpu.cores} cores</span>} data-host-identity>
      {used === null ? (
        <div className="big-null">
          <NullValue reason="usage unavailable" />
          <p className="sub">The host has not reported a CPU sample yet.</p>
        </div>
      ) : (
        <>
          <p className="big"><b>{used}</b><span className="u">%</span><span className="sub">now · current sample</span></p>
          <div className="meter-track" role="meter" aria-label="CPU" aria-valuemin={0} aria-valuemax={100} aria-valuenow={used} aria-valuetext={`${used}%`}>
            <div className="meter-fill" style={{ inlineSize: `${used}%` }} />
          </div>
        </>
      )}
    </Tile>
  );
}

export function HostDetailView({ id }: { id: string }) {
  const inventory = useInventory();
  const data = useResource(async api => {
    const epoch = inventory.epoch();
    const [host, vms, activity] = await Promise.all([api.getHost(id), api.listVms(), api.listActivity({ hostId: id, limit: 5 })]);
    inventory.put(epoch, { vms });
    return { host, vms: vms.filter(v => v.hostId === id), activity: activity.entries };
  }, [id]);
  const title = data.status === 'ready' ? data.data.host.name : id;
  const heading = usePageHeading(title, data.status);
  const now = useNow(30_000);

  return (
    <section className="page" aria-labelledby="page-title">
      {data.status !== 'ready' ? (
        <header className="page-head">
          <div className="page-title">
            <div className="eyebrow"><BackLink to={href({ name: 'hosts' })}>Hosts</BackLink></div>
            <h1 id="page-title" tabIndex={-1} ref={heading}>{title}</h1>
          </div>
        </header>
      ) : null}
      <Loaded resource={data} label="this host">
        {({ host, vms, activity }) => {
          const allocated = allocation(vms);
          const memPct = percent(host.memory.usedBytes, host.memory.totalBytes);
          const diskUsed = host.disk.totalBytes - host.disk.freeBytes;
          const offline = host.reachability === 'offline';
          return (
            <div className="bento">
              <section className="tile s5 hero-tile" aria-labelledby="page-title" data-host-identity>
                <BackLink to={href({ name: 'hosts' })}>Hosts</BackLink>
                <h1 id="page-title" className="hero-name" tabIndex={-1} ref={heading}>{host.name}</h1>
                <p className="meta">
                  <OsBadge os={host.os} />
                  <span>{host.osVersion}</span>
                  <ArchBadge arch={host.arch} />
                  <span className="dot" aria-hidden="true">·</span>
                  <HostStatus reachability={host.reachability} />
                  <span className="dot" aria-hidden="true">·</span>
                  <span>seen <time dateTime={host.lastSeenAt} title={formatDate(host.lastSeenAt)}>{formatAgo(host.lastSeenAt, now)}</time></span>
                </p>
                {host.reachabilityReason ? <p className="why"><Icon name={offline ? 'off' : 'warn'} />{host.reachabilityReason}</p> : null}
                <div className="hero-actions">
                  {offline
                    ? <span className="button button-disabled" aria-disabled="true" title={host.reachabilityReason ?? 'Host is offline'}><Icon name="term" />Run command</span>
                    : <a className="button button-primary" href={href({ name: 'terminal', hostId: host.id })}><Icon name="term" />Run command</a>}
                </div>
                <p className="note">One discovered tool at a time · not a shell</p>
              </section>

              <CpuTile host={host} />

              <Tile id="host-memory" icon="mem" title="Memory" className="s3" data-host-identity>
                <div className="ring-row">
                  <Ring label="Memory" pct={memPct} />
                  <div>
                    {host.memory.usedBytes === null
                      ? <NullValue reason="use unavailable" />
                      : <p className="ring-big">{formatBytes(host.memory.usedBytes)}</p>}
                    <p className="sub">host used · of {formatBytes(host.memory.totalBytes)}</p>
                  </div>
                </div>
                <p className="ring-legend"><span>allocated to VMs</span>{allocated.memoryBytes === null ? <NullValue reason="unknown" /> : <span className="mono">{formatBytes(allocated.memoryBytes)}</span>}</p>
              </Tile>

              <Tile id="host-disk" icon="disk" title="Disk" className="s3" data-host-identity>
                <div className="ring-row">
                  <Ring label="Disk" pct={percent(diskUsed, host.disk.totalBytes)} />
                  <div>
                    <p className="ring-big">{formatBytes(diskUsed)}</p>
                    <p className="sub">used of {formatBytes(host.disk.totalBytes)} · {formatBytes(host.disk.freeBytes)} free</p>
                  </div>
                </div>
                <p className="ring-legend">
                  <span>VM disk cap</span>
                  {host.disk.capBytes === null ? <span className="mono">none set</span> : <span className="mono">{formatBytes(allocated.diskBytes)} / {formatBytes(host.disk.capBytes)}</span>}
                </p>
              </Tile>

              <Tile id="host-facts" title="Facts" className="s6" data-host-identity>
                <Facts items={[
                  ['Operating system', `${OS_LABEL[host.os]} · ${host.osVersion}`],
                  ['Architecture', <ArchBadge key="a" arch={host.arch} />],
                  ['Processor', host.cpu.model],
                  ['Cores', String(host.cpu.cores)],
                  ['Availability', host.mode === 'always_on' ? 'Always on' : 'Opportunistic (used day to day)'],
                  ['Agent rights', host.agentRights === 'admin' ? 'Admin' : 'Standard privileges'],
                  ['Last seen', formatDate(host.lastSeenAt)],
                ]} />
              </Tile>

              <Tile id="host-vms" icon="vm" title={<>VMs on this host <span className="count">{vms.length}</span></>} className="s7"
                aside={vms.length > 0 ? <span>{vms.filter(v => v.kind === 'static').length} static · {vms.filter(v => v.kind === 'test').length} test</span> : undefined}>
                {vms.length === 0 ? <Empty title="No VMs on this host" /> : (
                  <>
                    <div className="vm-grid">{vms.map(v => <VmCard key={v.id} vm={v} now={now} />)}</div>
                    <div className="alloc alloc-wide" data-host-identity>
                      <p className="k">Allocated to VMs on {host.name}</p>
                      <CapBar label="vCPU" used={allocated.cpus} total={host.cpu.cores} format={n => `${n}`} reason="vCPU unknown" />
                      <CapBar label="RAM" used={allocated.memoryBytes} total={host.memory.totalBytes} format={formatBytes} reason="memory unknown" />
                      <CapBar label="VM disks" used={allocated.diskBytes} total={host.disk.capBytes} format={formatBytes} reason="unknown" />
                    </div>
                  </>
                )}
              </Tile>

              <Tile id="host-tools" icon="term" title={<>Tools <span className="count">{host.tools.length}</span></>} className="s5" aside={host.tools.length > 0 ? <span>discovered</span> : undefined}>
                {host.tools.length === 0 ? <Empty title="No tools discovered" /> : (
                  <ul className="tools" aria-labelledby="host-tools">
                    {host.tools.map(t => (
                      <li key={t.name}><code>{t.name}</code>{t.version ? <span className="tool-v">{t.version}</span> : <span className="tool-v">version unknown</span>}</li>
                    ))}
                  </ul>
                )}
              </Tile>

              <Tile id="host-activity" icon="act" title={`Activity on ${host.name}`} className="s12" aside={<a className="more" href={href({ name: 'activity' })}>All activity<Icon name="next" /></a>}>
                {activity.length === 0 ? <Empty title="No activity on this host" /> : <ActivityList entries={activity} label="Recent activity on this host" now={now} />}
              </Tile>
            </div>
          );
        }}
      </Loaded>
    </section>
  );
}
