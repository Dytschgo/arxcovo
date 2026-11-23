import { useState } from 'react';
import type { Arch, Vm } from '../../api/types';
import { useResource } from '../context';
import { OS_LABEL, formatDate } from '../format';
import { href } from '../router';
import { ArchBadge, Empty, Loaded, Page, Pill, SelectField, StatePill } from '../ui';

export function expiryLabel(vm: Vm): string {
  return vm.expiresAt === null ? 'Never (static)' : formatDate(vm.expiresAt);
}

export function VmsView() {
  const data = useResource(async api => {
    const [vms, hosts] = await Promise.all([api.listVms(), api.listHosts()]);
    return { vms, hostNames: new Map(hosts.map(h => [h.id, h.name])) };
  }, []);
  const [arch, setArch] = useState<'all' | Arch>('all');

  return (
    <Page title="Virtual machines" eyebrow="Cluster">
      <Loaded resource={data} label="virtual machines">
        {({ vms, hostNames }) => {
          if (vms.length === 0) return <Empty title="No virtual machines">Agents create test VMs from templates; they appear here with their architecture.</Empty>;
          const shown = arch === 'all' ? vms : vms.filter(v => v.arch === arch);
          return (
            <>
              <div className="toolbar">
                <SelectField label="Architecture" value={arch} onChange={v => setArch(v as 'all' | Arch)}>
                  <option value="all">All</option>
                  <option value="x86_64">x86_64</option>
                  <option value="arm64">arm64</option>
                </SelectField>
                <p className="summary" aria-live="polite">{shown.length} of {vms.length} VMs</p>
              </div>
              {shown.length === 0 ? <Empty title={`No ${arch} VMs`} /> : (
                <div className="table-scroll" tabIndex={0} role="region" aria-label="Virtual machines table">
                  <table className="table">
                    <thead>
                      <tr>
                        <th scope="col">Name</th>
                        <th scope="col">Arch</th>
                        <th scope="col">Guest</th>
                        <th scope="col">Host</th>
                        <th scope="col">State</th>
                        <th scope="col">Kind</th>
                        <th scope="col">Expires</th>
                      </tr>
                    </thead>
                    <tbody>
                      {shown.map(vm => (
                        <tr key={vm.id}>
                          <th scope="row"><a href={href({ name: 'vm', id: vm.id })}>{vm.name}</a></th>
                          <td><ArchBadge arch={vm.arch} /></td>
                          <td>{OS_LABEL[vm.guestOs]}<span className="cell-sub">{vm.guestOsVersion}</span></td>
                          <td><a href={href({ name: 'host', id: vm.hostId })}>{hostNames.get(vm.hostId) ?? vm.hostId}</a></td>
                          <td>
                            <span className="cell-pills">
                              <StatePill value={vm.state} />
                              {vm.state === 'running' && !vm.ready ? <Pill tone="warn">not ready</Pill> : null}
                              {vm.ready ? <Pill tone="ok">ready</Pill> : null}
                            </span>
                          </td>
                          <td>{vm.kind === 'static' ? 'Static' : 'Test'}</td>
                          <td>{expiryLabel(vm)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          );
        }}
      </Loaded>
    </Page>
  );
}
