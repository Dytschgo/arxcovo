import type { ReactNode } from 'react';
import type { Host, Vm } from '../../api/types';
import { useInventory, useResource } from '../context';
import { OS_LABEL, formatAgo, formatBytes, formatDate, formatTimeLeft, incompatibility } from '../format';
import { Icon, OsGlyph } from '../icons';
import { href } from '../router';
import { ArchBadge, BackLink, Empty, ErrorState, Facts, Loaded, Loading, Maybe, NullValue, OsBadge, Tile, VmStatus, useNow, usePageHeading } from '../ui';
import { ConsoleTile } from './ConsoleTile';
import type { HostState } from '../derive';
import { expiryLabel } from './VmsView';

// VM detail with the console tile (docs/design/night-orbit/vm.html). Static VM
// health, backups and publishing are not in the API and are not drawn; the
// compatible-hosts list only explains where the VM could run (migration is
// not shipped).

/** A size against what the host has, with a thin bar when both are known. */
function Res({ label, value, of, ratio }: { label: string; value: ReactNode; of: string | null; ratio: number | null }) {
  return (
    <div className="res">
      <dt>{label}</dt>
      <dd>{value}</dd>
      {of ? <span className="res-of">{of}</span> : null}
      <span className="res-bar" aria-hidden="true">{ratio === null ? null : <i style={{ inlineSize: `${Math.min(100, Math.max(0, ratio * 100))}%` }} />}</span>
    </div>
  );
}

function Resources({ vm, host }: { vm: Vm; host: Host | undefined }) {
  const name = host?.name ?? vm.hostId;
  return (
    <Tile id="vm-size" icon="cpu" title="Resources">
      <dl className="facts res-list" data-host-identity>
        <Res label="vCPUs" value={<Maybe value={vm.cpus} reason="unknown">{n => String(n)}</Maybe>}
          of={host ? `of ${host.cpu.cores} ${name} cores` : null} ratio={host && vm.cpus !== null ? vm.cpus / host.cpu.cores : null} />
        <Res label="Memory" value={<Maybe value={vm.memoryBytes} reason="unknown">{formatBytes}</Maybe>}
          of={host ? `of ${formatBytes(host.memory.totalBytes)}` : null} ratio={host && vm.memoryBytes !== null ? vm.memoryBytes / host.memory.totalBytes : null} />
        <Res label="Disk" value={formatBytes(vm.diskBytes)}
          of={host ? (host.disk.capBytes === null ? `of ${formatBytes(host.disk.totalBytes)} disk` : `of ${formatBytes(host.disk.capBytes)} VM cap`) : null}
          ratio={host ? vm.diskBytes / (host.disk.capBytes ?? host.disk.totalBytes) : null} />
      </dl>
    </Tile>
  );
}

function expiryPhrase(vm: Vm, now: number): string {
  if (vm.expiresAt === null) return 'never expires';
  const left = formatTimeLeft(vm.expiresAt, now);
  return left.startsWith('in ') ? `expires ${left}` : left;
}

export function VmPage({ id, console: live }: { id: string; console: boolean }) {
  // The VM and the host list load separately: the host list only adds
  // context (host name, capacity, compatibility, the command shortcut), so its
  // failure or delay never blocks the VM page or its console.
  const inventory = useInventory();
  const data = useResource(api => api.getVm(id), [id]);
  const hostList = useResource(async api => {
    const epoch = inventory.epoch();
    const list = await api.listHosts();
    inventory.put(epoch, { hosts: list });
    return list;
  }, []);
  const name = data.status === 'ready' ? data.data.name : id;
  const title = live ? `Console · ${name}` : name;
  const heading = usePageHeading(title, data.status);
  const now = useNow(30_000);

  return (
    <section className="page" aria-labelledby="page-title">
      {data.status !== 'ready' ? (
        <header className="page-head">
          <div className="page-title">
            <div className="eyebrow"><BackLink to={href({ name: 'vms' })}>VMs</BackLink></div>
            <h1 id="page-title" tabIndex={-1} ref={heading}>{title}</h1>
          </div>
        </header>
      ) : null}
      <Loaded resource={data} label="this VM">
        {vm => {
          const hosts = hostList.status === 'ready' ? hostList.data : null;
          const host = hosts?.find(h => h.id === vm.hostId);
          const hostState: HostState = hostList.status === 'loading' ? { status: 'pending' } : hostList.status === 'error' ? { status: 'unavailable' } : { status: 'ready', host };
          const hostName = host?.name ?? vm.hostId;
          const offline = host?.reachability === 'offline';
          return (
            <div className="bento">
              <section className="tile s12 vm-hero" aria-labelledby="page-title">
                <div className="vm-hero-main">
                  <BackLink to={href({ name: 'vms' })}>VMs</BackLink>
                  <h1 id="page-title" className="vm-name" tabIndex={-1} ref={heading}>
                    <span className="kind" data-kind={vm.kind} aria-hidden="true" />
                    {live ? <><span className="visually-hidden">Console ·</span>{' '}</> : null}
                    {vm.name}
                  </h1>
                  <p className="meta">
                    <span className="kindw">{vm.kind}</span>
                    <span className="dot" aria-hidden="true">·</span>
                    <OsBadge os={vm.guestOs} />
                    <span>{vm.guestOsVersion}</span>
                    <ArchBadge arch={vm.arch} />
                    <span className="dot" aria-hidden="true">·</span>
                    <span>on <a href={href({ name: 'host', id: vm.hostId })}>{hostName}</a></span>
                    <span className="dot" aria-hidden="true">·</span>
                    <VmStatus state={vm.state} />
                    <span className="ready" data-bad={!vm.ready && vm.state === 'running' ? 'true' : undefined}><Icon name={vm.ready ? 'check' : 'warn'} />{vm.ready ? 'guest ready' : 'guest not ready'}</span>
                    <span className="dot" aria-hidden="true">·</span>
                    <span className="mono" title={expiryLabel(vm)}>{expiryPhrase(vm, now)}</span>
                  </p>
                </div>
                <div className="vm-hero-side">
                  <p className="tpl">template <b className="mono">{vm.template.id}</b> <span className="arch">v{vm.template.version}</span></p>
                  {/* The shortcut needs the VM's own host in the loaded list: an
                      unknown host (left the list, or the list did not load) gets
                      no shortcut, so a command can never land on another host. */}
                  {host === undefined
                    ? null
                    : offline
                      ? <span className="button button-disabled" aria-disabled="true" title={host.reachabilityReason ?? 'Host is offline'}><Icon name="term" />Run command on {hostName}</span>
                      : <a className="button" href={href({ name: 'terminal', hostId: host.id })}><Icon name="term" />Run command on {hostName}</a>}
                </div>
              </section>

              {/* A stable key: host metadata arriving never remounts a live viewer
                  (the gate keeps the viewer out until the host is known, and a
                  later host-list retry keeps a viewer that is already up). */}
              <ConsoleTile key={vm.id} vm={vm} hostState={hostState} live={live} onRecheck={() => { data.reload(); hostList.reload(); }} />
              <div className="s4 stack">
                <Resources vm={vm} host={host} />
                <Tile id="vm-facts" title="Overview">
                <Facts items={[
                  ['Guest', <span key="g" className="guest"><OsGlyph os={vm.guestOs} />{OS_LABEL[vm.guestOs]} · {vm.guestOsVersion}</span>],
                  ['Architecture', <ArchBadge key="a" arch={vm.arch} />],
                  ['Template', `${vm.template.id} v${vm.template.version}`],
                  ['Host', <a key="h" href={href({ name: 'host', id: vm.hostId })}>{hostName}</a>],
                  ['Expires', expiryLabel(vm)],
                ]} />
              </Tile>
              </div>

              <Tile id="vm-snapshots" icon="cam" title={<>Snapshots <span className="count">{vm.snapshots.length}</span></>} className="s6">
                {vm.snapshots.length === 0 ? <Empty title="No snapshots yet" /> : (
                  <ul className="snaps">
                    {vm.snapshots.map(s => (
                      <li key={s.id}>
                        <Icon name="cam" />
                        <div className="snap-main">
                          <b className="mono">{s.name}</b>
                          {s.createdAt === null ? <NullValue reason="time unknown" /> : <time dateTime={s.createdAt} title={formatDate(s.createdAt)}>{formatAgo(s.createdAt, now)}</time>}
                        </div>
                        {s.sizeBytes === null ? <NullValue reason="size unknown" /> : <span className="mono snap-size">{formatBytes(s.sizeBytes)}</span>}
                      </li>
                    ))}
                  </ul>
                )}
              </Tile>

              <Tile id="vm-hosts" icon="hosts" title="Compatible hosts" className="s6">
                {hostList.status === 'loading' ? <Loading label="Loading the host list…" /> : null}
                {hostList.status === 'error' && hostList.error.code !== 'unauthorized'
                  ? <ErrorState message={`Could not load the host list. ${hostList.error.message}`} onRetry={hostList.reload} />
                  : null}
                {hosts !== null && hosts.length === 0 ? <Empty title="No hosts in the cluster" /> : null}
                {hosts !== null && host === undefined && hosts.length > 0 ? <p className="note"><Icon name="warn" />This VM's host, {vm.hostId}, is not in the host list.</p> : null}
                {hosts !== null && hosts.length > 0 ? <ul className="compat" aria-labelledby="vm-hosts" data-host-identity>
                  {hosts.map(h => {
                    const reason = incompatibility(vm, h);
                    const current = h.id === vm.hostId;
                    return (
                      <li key={h.id} data-compatible={reason === null} aria-disabled={reason !== null || undefined}>
                        <OsBadge os={h.os} />
                        <b className="mono">{h.name}</b>
                        <ArchBadge arch={h.arch} />
                        <span className="compat-why">
                          <Icon name={current ? 'run' : reason === null ? 'check' : 'ban'} />
                          {reason ?? (current ? 'Current host' : 'Compatible')}
                        </span>
                      </li>
                    );
                  })}
                </ul> : null}
                <p className="note">Where this VM could run. Moving VMs between hosts ships after v1.</p>
              </Tile>
            </div>
          );
        }}
      </Loaded>
    </section>
  );
}

export function VmDetailView({ id }: { id: string }) {
  return <VmPage id={id} console={false} />;
}
