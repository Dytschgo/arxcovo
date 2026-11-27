import { useCallback, useState } from 'react';
import { summarizeCpuHistory } from '../../api/cpuHistory';
import { ApiError, CPU_HISTORY_RANGES, type ArxApi, type CpuHistoryRange, type Host } from '../../api/types';
import { useInventory, useResource } from '../context';
import { CpuTrace, RANGE_WORDS, hasSamples, refreshFor, usePolled } from '../cpuHistory';
import { allocation } from '../derive';
import { OS_LABEL, formatAgo, formatBytes, formatDate, percent } from '../format';
import { Icon } from '../icons';
import { href } from '../router';
import { ArchBadge, BackLink, CapBar, Empty, Facts, HostStatus, Loaded, NullValue, OsBadge, Tile, VmCard, useNow, usePageHeading } from '../ui';
import { ActivityList } from './ActivityList';

// Host detail as a Night Orbit bento (docs/design/night-orbit/host.html).
// CPU shows the current sample and, below it, the CPU history trace for a
// chosen range with avg and peak over the sampled buckets. History loads on its
// own and never holds up the page. The two-handle time window, remote desktop
// and the disk reserve are not drawn.

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

const RANGES = Object.keys(CPU_HISTORY_RANGES) as CpuHistoryRange[];
const AXIS: Record<CpuHistoryRange, string[]> = {
  '1h': ['-60 min', '-45', '-30', '-15', 'now'],
  '6h': ['-6 h', '-4.5 h', '-3 h', '-1.5 h', 'now'],
  '24h': ['-24 h', '-18 h', '-12 h', '-6 h', 'now'],
};

/** A percentage readout; null (no sampled bucket) is hatched, never 0. */
function Readout({ label, value }: { label: string; value: number | null }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value === null ? <NullValue reason="usage unavailable" /> : `${Math.round(value)}%`}</dd>
    </div>
  );
}

/**
 * CPU history for one host: range selector, trace and avg/peak. Each response
 * is checked against the host and range it was asked for, and a response for
 * an earlier host or range is dropped (`usePolled`), so rapid switching never
 * shows a stale trace.
 */
export function CpuHistoryPanel({ host }: { host: Host }) {
  const [range, setRange] = useState<CpuHistoryRange>('24h');
  const load = useCallback(async (api: ArxApi) => {
    const history = await api.getCpuHistory(host.id, range);
    if (history.hostId !== host.id || history.range !== range) throw new ApiError('bad_response', 'CPU history did not match the requested host and range.');
    return history;
  }, [host.id, range]);
  const state = usePolled(`${host.id}:${range}`, load, refreshFor(range));
  const ready = state.status === 'ready' ? state.data : null;
  const summary = ready ? summarizeCpuHistory(ready.samples) : null;

  return (
    <div className="cpu-hist" data-range={range} aria-busy={state.status === 'loading'}>
      <div className="hist-head">
        <p className="hist-k">History · {RANGE_WORDS[range]}</p>
        {summary ? (
          <dl className="hist-read">
            <Readout label="avg" value={summary.avg} />
            <Readout label="peak" value={summary.peak} />
          </dl>
        ) : null}
        <div className="seg" role="group" aria-label="CPU history range">
          {RANGES.map(r => <button key={r} type="button" aria-pressed={range === r} onClick={() => setRange(r)}>{r}</button>)}
        </div>
      </div>
      {state.status === 'loading' ? (
        <div className="cpu-chart chart-wait"><p className="hist-note">Loading CPU history…</p></div>
      ) : state.status === 'error' ? (
        state.error.code === 'unauthorized' ? null : (
          <div className="cpu-chart chart-na">
            <NullValue reason="history unavailable" />
            <p className="hist-note">{state.error.message}</p>
          </div>
        )
      ) : !hasSamples(state.data) ? (
        <div className="cpu-chart chart-na">
          <NullValue reason="no samples" />
          <p className="hist-note">arxd has no CPU samples for {host.name} in the {RANGE_WORDS[range]}.</p>
        </div>
      ) : (
        <>
          <CpuTrace history={state.data} os={host.os} className="cpu-chart" grid />
          <div className="cpu-axis" aria-hidden="true">{AXIS[range].map(t => <span key={t}>{t}</span>)}</div>
          {summary && summary.missing > 0
            ? <p className="hist-note"><span className="na" aria-hidden="true" />{summary.missing} of {state.data.samples.length} buckets missing · avg and peak cover sampled buckets only</p>
            : null}
        </>
      )}
    </div>
  );
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
      <CpuHistoryPanel host={host} />
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
