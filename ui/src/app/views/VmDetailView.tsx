import { useResource } from '../context';
import { OS_LABEL, formatBytes, formatDate, incompatibility } from '../format';
import { href } from '../router';
import { ArchBadge, Empty, Facts, Loaded, Page, Pill, VmStatus } from '../ui';
import { expiryLabel } from './VmsView';

export function VmDetailView({ id }: { id: string }) {
  const data = useResource(async api => {
    const [vm, hosts] = await Promise.all([api.getVm(id), api.listHosts()]);
    return { vm, hosts };
  }, [id]);
  const title = data.status === 'ready' ? data.data.vm.name : id;
  return (
    <Page title={title} eyebrow={<a href={href({ name: 'vms' })}>Virtual machines</a>} actions={data.status === 'ready' ? <a className="button button-primary" href={href({ name: 'console', id })}>Open console</a> : undefined}>
      <Loaded resource={data} label="this VM">
        {({ vm, hosts }) => {
          const host = hosts.find(h => h.id === vm.hostId);
          return (
            <div className="detail">
              <div className="detail-badges">
                <VmStatus state={vm.state} />
                <ArchBadge arch={vm.arch} />
                <Pill tone={vm.ready ? 'ok' : 'warn'}>{vm.ready ? 'guest ready' : 'guest not ready'}</Pill>
                <Pill tone="muted">{vm.kind === 'static' ? 'static VM' : 'test VM'}</Pill>
              </div>
              <div className="panels">
                <section className="panel" aria-labelledby="vm-facts">
                  <h2 id="vm-facts">Overview</h2>
                  <Facts items={[
                    ['Guest', `${OS_LABEL[vm.guestOs]} · ${vm.guestOsVersion}`],
                    ['Architecture', <ArchBadge key="a" arch={vm.arch} />],
                    ['Template', `${vm.template.id} v${vm.template.version}`],
                    ['Host', <a key="h" href={href({ name: 'host', id: vm.hostId })}>{host?.name ?? vm.hostId}</a>],
                    ['Expires', expiryLabel(vm)],
                  ]} />
                </section>
                <section className="panel" aria-labelledby="vm-size">
                  <h2 id="vm-size">Size</h2>
                  <Facts items={[
                    ['vCPUs', vm.cpus === null ? '—' : String(vm.cpus)],
                    ['Memory', vm.memoryBytes === null ? '—' : formatBytes(vm.memoryBytes)],
                    ['Disk', formatBytes(vm.diskBytes)],
                  ]} />
                </section>
              </div>
              <section className="panel" aria-labelledby="vm-snapshots">
                <h2 id="vm-snapshots">Snapshots <span className="count">{vm.snapshots.length}</span></h2>
                {vm.snapshots.length === 0 ? <Empty title="No snapshots yet" /> : (
                  <ul className="rows">
                    {vm.snapshots.map(s => (
                      <li key={s.id} className="row">
                        <span className="row-title">{s.name}</span>
                        {s.createdAt === null ? <span>—</span> : <time dateTime={s.createdAt}>{formatDate(s.createdAt)}</time>}
                        <span>{s.sizeBytes === null ? '—' : formatBytes(s.sizeBytes)}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </section>
              <section className="panel" aria-labelledby="vm-hosts">
                <h2 id="vm-hosts">Compatible hosts</h2>
                <p className="panel-note">Where this VM could run or migrate. Incompatible hosts are greyed out with the reason.</p>
                <ul className="rows" aria-labelledby="vm-hosts">
                  {hosts.map(h => {
                    const reason = incompatibility(vm, h);
                    return (
                      <li key={h.id} className="row" data-compatible={reason === null} aria-disabled={reason !== null || undefined}>
                        <span className="row-title">{h.name}{h.id === vm.hostId ? <span className="row-tag"> (current)</span> : null}</span>
                        <ArchBadge arch={h.arch} />
                        <span className="row-reason">{reason ?? 'Compatible'}</span>
                      </li>
                    );
                  })}
                </ul>
              </section>
            </div>
          );
        }}
      </Loaded>
    </Page>
  );
}
