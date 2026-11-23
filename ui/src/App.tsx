import { useEffect, useRef } from 'react';
import type { ArxApi } from './api/types';
import { AppProviders, useApi, useSession } from './app/context';
import { href, useRoute, type Route } from './app/router';
import { NEXT_THEME, useTheme } from './app/theme';
import { Empty, Loading, Page } from './app/ui';
import { ActivityView } from './app/views/ActivityView';
import { ConsoleView } from './app/views/ConsoleView';
import { HostDetailView } from './app/views/HostDetailView';
import { HostsView } from './app/views/HostsView';
import { LoginView } from './app/views/LoginView';
import { TerminalView } from './app/views/TerminalView';
import { VmDetailView } from './app/views/VmDetailView';
import { VmsView } from './app/views/VmsView';

const NAV: { label: string; route: Route; matches: Route['name'][] }[] = [
  { label: 'Hosts', route: { name: 'hosts' }, matches: ['hosts', 'host'] },
  { label: 'VMs', route: { name: 'vms' }, matches: ['vms', 'vm', 'console'] },
  { label: 'Activity', route: { name: 'activity' }, matches: ['activity'] },
  { label: 'Terminal', route: { name: 'terminal', hostId: null }, matches: ['terminal'] },
];

export function DemoBanner() {
  return (
    <div className="demo-banner" role="note" aria-label="Demo data">
      <strong>Demo data.</strong> This dashboard is using the built-in stand-in adapter, not a real arxd. Nothing you see is live.
    </div>
  );
}

function View({ route }: { route: Route }) {
  switch (route.name) {
    case 'hosts': return <HostsView />;
    case 'host': return <HostDetailView id={route.id} />;
    case 'vms': return <VmsView />;
    case 'vm': return <VmDetailView id={route.id} />;
    case 'console': return <ConsoleView id={route.id} />;
    case 'activity': return <ActivityView />;
    case 'terminal': return <TerminalView hostId={route.hostId} />;
    case 'login': return <HostsView />;
    case 'not_found':
      return (
        <Page title="Page not found">
          <Empty title="Nothing lives at this address."><a href={href({ name: 'hosts' })}>Go to hosts</a></Empty>
        </Page>
      );
  }
}

function Shell() {
  const api = useApi();
  const { state, logout } = useSession();
  const route = useRoute();
  const [theme, setTheme] = useTheme();
  const main = useRef<HTMLElement>(null);

  useEffect(() => {
    if (state.status === 'authenticated' && route.name === 'login') window.location.replace(href({ name: 'hosts' }));
  }, [state.status, route.name]);

  const themeToggle = (
    <button type="button" className="button button-quiet theme-toggle" onClick={() => setTheme(NEXT_THEME[theme])} aria-label={`Theme: ${theme}. Switch to ${NEXT_THEME[theme]}`}>
      <span aria-hidden="true">{theme === 'dark' ? '☾' : theme === 'light' ? '☀' : '◐'}</span>
      <span className="theme-label">{theme}</span>
    </button>
  );

  let body;
  if (state.status === 'checking') body = <div className="login"><Loading label="Checking your session…" /></div>;
  else if (state.status === 'anonymous') body = <LoginView notice={state.notice} error={state.error} />;
  else {
    body = (
      <div className="frame">
        <nav className="sidebar" aria-label="Main">
          <a className="brand" href={href({ name: 'hosts' })}><span className="brand-mark" aria-hidden="true" />Arxcovo</a>
          <ul className="nav">
            {NAV.map(item => {
              const current = item.matches.includes(route.name);
              return (
                <li key={item.label}>
                  <a href={href(item.route)} aria-current={current ? 'page' : undefined}>{item.label}</a>
                </li>
              );
            })}
          </ul>
          <div className="sidebar-foot">
            {themeToggle}
            <span className="user" title="Signed in">{state.session.user.name}</span>
            <button type="button" className="button button-quiet" onClick={() => void logout()}>Sign out</button>
          </div>
        </nav>
        <main id="main" ref={main} tabIndex={-1}>
          <View route={route} />
        </main>
      </div>
    );
  }

  return (
    <div className="app">
      <button type="button" className="skip-link" onClick={() => (main.current ?? document.querySelector<HTMLElement>('main, form'))?.focus()}>Skip to content</button>
      {api.kind === 'standin' ? <DemoBanner /> : null}
      {body}
    </div>
  );
}

export function App({ api }: { api: ArxApi }) {
  return (
    <AppProviders api={api}>
      <Shell />
    </AppProviders>
  );
}
