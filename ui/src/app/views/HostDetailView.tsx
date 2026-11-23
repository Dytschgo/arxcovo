import { useResource } from '../context';
import { OS_LABEL, formatDate } from '../format';
import { href } from '../router';
import { ArchBadge, Empty, Facts, Loaded, Page, StatePill } from '../ui';
import { ActivityList } from './ActivityList';
import { HostMeters } from './HostsView';

export function HostDetailView({ id }: { id: string }) {
  const data = useResource(async api => {
    const [host, vms, activity] = await Promise.all([api.getHost(id), api.listVms(), api.listActivity({ hostId: id, limit: 5 })]);
    return { host, vms: vms.filter(v => v.hostId === id), activity: activity.entries };
  }, [id]);
  const title = data.status === 'ready' ? data.data.host.name : id;
  return (
    <Page title={title} eyebrow={<a href={href({ name: 'hosts' })}>Hosts</a>} actions={data.status === 'ready' ? (
      data.data.host.reachability === 'offline'
        ? <span className="button button-disabled" aria-disabled="true" title={data.data.host.reachabilityReason ?? 'Host is offline'}>Open terminal</span>
        : <a className="button button-primary" href={href({ name: 'terminal', hostId: id })}>Open terminal</a>
    ) : undefined}>
      <Loaded resource={data} label="this host">
        {({ host, vms, activity }) => (
          <div className="detail">
            <div className="detail-badges">
              <StatePill value={host.reachability} />
              <ArchBadge arch={host.arch} />
              {host.reachabilityReason ? <span className="card-reason">{host.reachabilityReason}</span> : null}
            </div>
            <div className="panels">
              <section className="panel" aria-labelledby="host-facts">
                <h2 id="host-facts">Overview</h2>
                <Facts items={[
                  ['Operating system', `${OS_LABEL[host.os]} · ${host.osVersion}`],
                  ['Architecture', <ArchBadge key="a" arch={host.arch} />],
                  ['Processor', host.cpu.model],
                  ['Mode', host.mode === 'always_on' ? 'Always on (dedicated)' : 'Opportunistic (daily use)'],
                  ['Agent rights', host.agentRights === 'admin' ? 'Admin' : 'Standard privileges'],
                  ['Last seen', formatDate(host.lastSeenAt)],
                ]} />
              </section>
              <section className="panel" aria-labelledby="host-resources">
                <h2 id="host-resources">Resources</h2>
                <HostMeters host={host} />
              </section>
            </div>
            <section className="panel" aria-labelledby="host-tools">
              <h2 id="host-tools">Tools <span className="count">{host.tools.length}</span></h2>
              {host.tools.length === 0 ? <Empty title="No tools discovered" /> : (
                <ul className="chips" aria-labelledby="host-tools">
                  {host.tools.map(t => <li key={t.name} className="chip"><code>{t.name}</code>{t.version ? <span className="chip-version">{t.version}</span> : null}</li>)}
                </ul>
              )}
            </section>
            <section className="panel" aria-labelledby="host-vms">
              <h2 id="host-vms">VMs on this host <span className="count">{vms.length}</span></h2>
              {vms.length === 0 ? <Empty title="No VMs on this host" /> : (
                <ul className="rows">
                  {vms.map(v => (
                    <li key={v.id} className="row">
                      <a href={href({ name: 'vm', id: v.id })}>{v.name}</a>
                      <ArchBadge arch={v.arch} />
                      <StatePill value={v.state} />
                    </li>
                  ))}
                </ul>
              )}
            </section>
            <section className="panel" aria-labelledby="host-activity">
              <h2 id="host-activity">Recent activity</h2>
              {activity.length === 0 ? <Empty title="No activity on this host" /> : <ActivityList entries={activity} label="Recent activity on this host" />}
            </section>
          </div>
        )}
      </Loaded>
    </Page>
  );
}
