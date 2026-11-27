import { useState, type CSSProperties } from 'react';
import type { ArxApi, CpuHistory, Host, VmKind } from '../../api/types';
import { useInventory, useResource } from '../context';
import { CpuTrace, hasSamples, usePolled, type Polled } from '../cpuHistory';
import { boardTotals, groupBoard, type Allocation, type BoardTotals } from '../derive';
import { formatBytes } from '../format';
import { Icon } from '../icons';
import { href } from '../router';
import { ArchBadge, CapBar, Empty, HostStatus, Loaded, NullValue, OsBadge, Page, VmCard, useNow } from '../ui';

// The v1 Hosts view is the Night Orbit board: one column per host holding its
// VM cards (docs/design/night-orbit/board.html). Migration is not shipped, so
// cards don't move and there is no Move menu or drain; there is no join entry
// because the dashboard API has no join-code endpoint. Each column head carries
// a 24 h CPU trace from one batch request that never holds up the board.

type KindFilter = VmKind | 'all';
const KIND_FILTERS: { value: KindFilter; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'test', label: 'Test' },
  { value: 'static', label: 'Static' },
];

/** CPU now: the latest sample, or hatched "usage unavailable" when the host sends null. */
export function CpuNow({ host }: { host: Host }) {
  const used = host.cpu.usedPercent;
  return (
    <div className="cpu-now">
      <span className="k">CPU now</span>
      {used === null
        ? <NullValue reason="usage unavailable" />
        : <span className="cpu-v"><b>{Math.round(used)}</b><span className="u">%</span></span>}
    </div>
  );
}

/** vCPU and RAM the host's VMs ask for, against the host's cores and memory. */
export function Allocated({ host, allocated, label }: { host: Host; allocated: Allocation; label: string }) {
  return (
    <div className="alloc">
      <p className="k">{label}</p>
      <CapBar label="vCPU" used={allocated.cpus} total={host.cpu.cores} format={String} reason="vCPU unknown" />
      <CapBar label="RAM" used={allocated.memoryBytes} total={host.memory.totalBytes} format={formatBytes} reason="memory unknown" />
      {allocated.unknown > 0
        ? <p className="alloc-note"><span className="na" aria-hidden="true" />{allocated.unknown === 1 ? '1 VM reports no size; not counted' : `${allocated.unknown} VMs report no size; not counted`}</p>
        : null}
    </div>
  );
}

const loadBoardHistory = async (api: ArxApi, signal: AbortSignal) => {
  const series = await api.listCpuHistory('24h', { signal });
  return new Map(series.filter(s => s.range === '24h').map(s => [s.hostId, s]));
};

/**
 * The column's 24 h trace. While the batch loads the space stays empty (no
 * layout shift); without history for this host (request failed, host not in
 * the batch, or every bucket null) the band is hatched, never a flat 0 line.
 */
export function BoardTrace({ host, history }: { host: Host; history: Polled<Map<string, CpuHistory>> }) {
  if (history.status === 'loading') return <div className="spark spark-wait" aria-hidden="true" />;
  const series = history.status === 'ready' ? history.data.get(host.id) : undefined;
  if (series && hasSamples(series)) return <CpuTrace history={series} os={host.os} className="spark" />;
  const why = series ? 'No CPU samples in the last 24 hours' : 'CPU history unavailable';
  return <div className="spark spark-na" data-null="true" title={why}><span className="visually-hidden">{why}</span></div>;
}

function Totals({ totals }: { totals: BoardTotals }) {
  return (
    <>
      <p className="hero" aria-live="polite">
        <span aria-hidden="true"><b>{totals.online}</b><span className="of">/{totals.hosts}</span><span className="w">hosts online</span></span>
        <span className="visually-hidden">{totals.online} of {totals.hosts} hosts online</span>
      </p>
      <dl className="grp">
        <dt>Hosts not online</dt>
        <dd><span><b>{totals.degraded}</b> degraded</span><span><b>{totals.offline}</b> offline</span></dd>
      </dl>
      <dl className="grp">
        <dt>VMs on all hosts, incl. offline</dt>
        <dd>
          <span><b>{totals.vms}</b> total</span>
          <span><b>{totals.running}</b> running</span>
          <span><b>{totals.test}</b> test</span>
          <span><b>{totals.static}</b> static</span>
        </dd>
      </dl>
    </>
  );
}

const REASON_ICON = { online: 'check', degraded: 'warn', offline: 'off' } as const;

export function HostsView() {
  const inventory = useInventory();
  const data = useResource(async api => {
    const epoch = inventory.epoch();
    const [hosts, vms] = await Promise.all([api.listHosts(), api.listVms()]);
    inventory.put(epoch, { hosts, vms });
    return { hosts, vms };
  }, []);
  const history = usePolled('board:24h', loadBoardHistory, 60_000);
  const [kind, setKind] = useState<KindFilter>('all');
  const now = useNow(30_000);

  return (
    <Page title="Hosts" hideTitle>
      <Loaded resource={data} label="hosts">
        {({ hosts, vms }) => {
          // Only an empty cluster gets the empty screen: VMs whose host is not
          // listed (even when no host is) still show, with their totals.
          if (hosts.length === 0 && vms.length === 0) return <Empty title="No hosts yet">Install arxd on a computer and join it with a one-time join code.</Empty>;
          const board = groupBoard(hosts, vms, kind);
          return (
            <>
              <header className="board-head">
                <Totals totals={boardTotals(hosts, vms)} />
                <div className="seg" role="group" aria-label="Show VMs">
                  {KIND_FILTERS.map(f => (
                    <button key={f.value} type="button" aria-pressed={kind === f.value} onClick={() => setKind(f.value)}>{f.label}</button>
                  ))}
                </div>
              </header>
              <p className="legend">
                <span><span className="kind" data-kind="test" aria-hidden="true" />test VM, expires</span>
                <span><span className="kind" data-kind="static" aria-hidden="true" />static VM, never expires</span>
                <span><span className="trace-key" aria-hidden="true" />CPU, last 24 h</span>
                <span><span className="sw" aria-hidden="true" />unavailable</span>
              </p>
              <div className="board" role="region" aria-label="Hosts board" tabIndex={0}>
                {board.columns.length > 0 ? <ul className="board-cols" aria-label="Hosts" style={{ '--cols': board.columns.length } as CSSProperties}>
                  {board.columns.map(({ host, vms: shown, allocated }) => (
                    // No fade-in entrance: text stays at full contrast in every frame.
                    <li key={host.id} className="col" data-reachability={host.reachability}>
                      <div className="col-h">
                        <div className="col-t">
                          <OsBadge os={host.os} />
                          <a className="hn" href={href({ name: 'host', id: host.id })}>{host.name}</a>
                        </div>
                        <div className="col-s">
                          <HostStatus reachability={host.reachability} />
                          <ArchBadge arch={host.arch} />
                        </div>
                        <p className="ver">{host.osVersion}</p>
                        <CpuNow host={host} />
                        <BoardTrace host={host} history={history} />
                        <Allocated host={host} allocated={allocated} label="allocated to VMs" />
                        {host.reachabilityReason
                          ? <p className="why"><Icon name={REASON_ICON[host.reachability]} />{host.reachabilityReason}</p>
                          : null}
                      </div>
                      <div className="col-b">
                        {shown.length === 0
                          ? <p className="col-empty">{kind === 'all' ? 'No VMs' : `No ${kind} VMs`}</p>
                          : shown.map(vm => <VmCard key={vm.id} vm={vm} now={now} />)}
                      </div>
                    </li>
                  ))}
                </ul> : null}
                {board.orphans.length > 0 ? (
                  <section className="col col-orphan" aria-labelledby="orphans">
                    <div className="col-h">
                      <h2 id="orphans" className="hn">Unknown host</h2>
                      <p className="ver">These VMs name a host that is not in the host list.</p>
                    </div>
                    <div className="col-b">{board.orphans.map(vm => <VmCard key={vm.id} vm={vm} now={now} />)}</div>
                  </section>
                ) : null}
              </div>
            </>
          );
        }}
      </Loaded>
    </Page>
  );
}
