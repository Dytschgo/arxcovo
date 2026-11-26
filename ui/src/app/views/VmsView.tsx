import { useId, useState } from 'react';
import { VM_STATES, type Arch, type Host, type Vm, type VmKind, type VmState } from '../../api/types';
import { useInventory, useResource } from '../context';
import { formatBytes, formatDate, formatTimeLeft } from '../format';
import { Icon, OsGlyph } from '../icons';
import { href } from '../router';
import { ArchBadge, Empty, HostStatus, KindMarker, Loaded, Maybe, Page, SelectField, VmStatus, useNow } from '../ui';

// VMs as a Night Orbit table (docs/design/night-orbit/vms.html). All filters
// are client-side over the one listVms() result.

export function expiryLabel(vm: Vm): string {
  return vm.expiresAt === null ? 'Never (static)' : formatDate(vm.expiresAt);
}

type KindFilter = VmKind | 'all';

/** The readiness cell: only meaningful while the guest runs. */
function Ready({ vm }: { vm: Vm }) {
  if (vm.ready) return <span className="ready"><Icon name="check" />yes</span>;
  if (vm.state === 'running') return <span className="ready" data-bad="true"><Icon name="warn" />not ready</span>;
  return <span className="ready" data-off="true">no</span>;
}

export function VmsView() {
  const inventory = useInventory();
  const data = useResource(async api => {
    const epoch = inventory.epoch();
    const [vms, hosts] = await Promise.all([api.listVms(), api.listHosts()]);
    inventory.put(epoch, { hosts, vms });
    return { vms, hosts: new Map(hosts.map(h => [h.id, h])) };
  }, []);
  const [kind, setKind] = useState<KindFilter>('all');
  const [arch, setArch] = useState<'all' | Arch>('all');
  const [hostId, setHostId] = useState('');
  const [state, setState] = useState<VmState | ''>('');
  const [name, setName] = useState('');
  const nameId = useId();
  const now = useNow(30_000);

  return (
    <Page title="Virtual machines" hideTitle>
      <Loaded resource={data} label="virtual machines">
        {({ vms, hosts }) => {
          if (vms.length === 0) return <Empty title="No virtual machines">Agents create test VMs from templates; they appear here with their architecture.</Empty>;
          const q = name.trim().toLowerCase();
          const shown = vms
            .filter(v => (kind === 'all' || v.kind === kind) && (arch === 'all' || v.arch === arch) && (hostId === '' || v.hostId === hostId) && (state === '' || v.state === state) && (q === '' || v.name.toLowerCase().includes(q)))
            .sort((a, b) => a.name.localeCompare(b.name));
          const count = (f: (v: Vm) => boolean) => vms.filter(f).length;
          const hostOf = (v: Vm): Host | undefined => hosts.get(v.hostId);
          return (
            <>
              <header className="vms-head">
                <p className="hero"><b>{vms.length}</b><span className="w">VMs</span></p>
                <p className="vms-sum">
                  <span><Icon name="run" /><b>{count(v => v.state === 'running')}</b> running</span>
                  <span><span className="kind" data-kind="test" aria-hidden="true" /><b>{count(v => v.kind === 'test')}</b> test</span>
                  <span><span className="kind" data-kind="static" aria-hidden="true" /><b>{count(v => v.kind === 'static')}</b> static</span>
                </p>
              </header>
              <div className="tile table-tile">
                <div className="toolbar">
                  <div className="seg" role="group" aria-label="Kind">
                    {(['all', 'test', 'static'] as const).map(k => (
                      <button key={k} type="button" aria-pressed={kind === k} onClick={() => setKind(k)}>
                        {k === 'all' ? 'All' : <><span className="kind" data-kind={k} aria-hidden="true" />{k === 'test' ? 'Test' : 'Static'}</>}
                        <span className="seg-n">{k === 'all' ? vms.length : count(v => v.kind === k)}</span>
                      </button>
                    ))}
                  </div>
                  <SelectField label="Architecture" value={arch} onChange={v => setArch(v as 'all' | Arch)}>
                    <option value="all">All</option>
                    <option value="x86_64">x86_64</option>
                    <option value="arm64">arm64</option>
                  </SelectField>
                  <SelectField label="Host" value={hostId} onChange={setHostId}>
                    <option value="">All hosts</option>
                    {[...hosts.values()].map(h => <option key={h.id} value={h.id}>{h.name}</option>)}
                  </SelectField>
                  <div className="filter-name">
                    <Icon name="search" />
                    <label htmlFor={nameId} className="visually-hidden">Filter by name</label>
                    <input id={nameId} type="search" placeholder="Filter by name" value={name} onChange={e => setName(e.target.value)} autoComplete="off" spellCheck={false} maxLength={128} />
                  </div>
                </div>
                <div className="chips-group state-chips" role="group" aria-label="State">
                  <span className="chips-k" aria-hidden="true">State</span>
                  {VM_STATES.map(s => (
                    <button key={s} type="button" className="fchip" aria-pressed={state === s} onClick={() => setState(cur => (cur === s ? '' : s))}>
                      <VmStatus state={s} /><span className="seg-n">{count(v => v.state === s)}</span>
                    </button>
                  ))}
                </div>
                <p className="summary" aria-live="polite">{shown.length} of {vms.length} VMs</p>
                {shown.length === 0 ? <Empty title="No VMs match these filters" /> : (
                  <div className="table-scroll" tabIndex={0} role="region" aria-label="Virtual machines table">
                    <table className="table">
                      <thead>
                        <tr>
                          <th scope="col">Kind</th>
                          <th scope="col" aria-sort="ascending">Name</th>
                          <th scope="col">Guest</th>
                          <th scope="col">Arch</th>
                          <th scope="col">Host</th>
                          <th scope="col">State</th>
                          <th scope="col">Ready</th>
                          <th scope="col">Expires</th>
                          <th scope="col" className="num">vCPU</th>
                          <th scope="col" className="num">RAM</th>
                          <th scope="col" className="num">Disk</th>
                          <th scope="col" className="num">Snap</th>
                        </tr>
                      </thead>
                      <tbody>
                        {shown.map(vm => {
                          const host = hostOf(vm);
                          return (
                            <tr key={vm.id} data-state={vm.state}>
                              <td><KindMarker kind={vm.kind} /></td>
                              <th scope="row"><a href={href({ name: 'vm', id: vm.id })}>{vm.name}</a></th>
                              <td><span className="guest"><OsGlyph os={vm.guestOs} />{vm.guestOsVersion}</span></td>
                              <td><ArchBadge arch={vm.arch} /></td>
                              <td>
                                <span className="host-cell">
                                  <a href={href({ name: 'host', id: vm.hostId })}>{host?.name ?? vm.hostId}</a>
                                  {host && host.reachability !== 'online' ? <HostStatus reachability={host.reachability} /> : null}
                                </span>
                              </td>
                              <td><VmStatus state={vm.state} /></td>
                              <td><Ready vm={vm} /></td>
                              <td className="mono exp" title={expiryLabel(vm)} data-soon={vm.expiresAt !== null && Date.parse(vm.expiresAt) - now < 3_600_000 || undefined}>
                                {vm.expiresAt === null ? 'never' : formatTimeLeft(vm.expiresAt, now)}
                              </td>
                              <td className="num"><Maybe value={vm.cpus} reason="unknown">{n => n}</Maybe></td>
                              <td className="num"><Maybe value={vm.memoryBytes} reason="unknown">{b => formatBytes(b)}</Maybe></td>
                              <td className="num">{formatBytes(vm.diskBytes)}</td>
                              <td className="num"><Icon name="cam" />{vm.snapshots.length}</td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                )}
                <p className="table-legend">
                  <span className="chips-k">Kind</span>
                  <span><span className="kind" data-kind="test" aria-hidden="true" />ring = test, expires</span>
                  <span><span className="kind" data-kind="static" aria-hidden="true" />diamond = static, never expires</span>
                  <span className="chips-k">Data</span>
                  <span><span className="sw" aria-hidden="true" />not reported</span>
                  <span>Countdowns keep running while a VM is stopped</span>
                </p>
              </div>
              <p className="foot-note">VMs are created by agents over MCP; agents cannot delete static VMs.</p>
            </>
          );
        }}
      </Loaded>
    </Page>
  );
}
