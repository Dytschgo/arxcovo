import { useEffect, useState } from 'react';

// Hash routing works the same from Vite, a static server, arxd and Electron's
// app:// protocol, and in-page hash changes never trigger a navigation.
export type Route =
  | { name: 'login' }
  | { name: 'hosts' }
  | { name: 'host'; id: string }
  | { name: 'vms' }
  | { name: 'vm'; id: string }
  | { name: 'console'; id: string }
  | { name: 'activity' }
  | { name: 'terminal'; hostId: string | null }
  | { name: 'not_found'; path: string };

const SEGMENT = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;

export function parseRoute(hash: string): Route {
  const path = hash.replace(/^#/, '').replace(/\/+$/, '') || '/hosts';
  const parts = path.split('/').filter(Boolean);
  const id = parts[1] !== undefined && SEGMENT.test(parts[1]) ? parts[1] : null;
  switch (parts[0]) {
    case 'login':
      if (parts.length === 1) return { name: 'login' };
      break;
    case 'hosts':
      if (parts.length === 1) return { name: 'hosts' };
      if (parts.length === 2 && id) return { name: 'host', id };
      break;
    case 'vms':
      if (parts.length === 1) return { name: 'vms' };
      if (parts.length === 2 && id) return { name: 'vm', id };
      if (parts.length === 3 && id && parts[2] === 'console') return { name: 'console', id };
      break;
    case 'activity':
      if (parts.length === 1) return { name: 'activity' };
      break;
    case 'terminal':
      if (parts.length === 1) return { name: 'terminal', hostId: null };
      if (parts.length === 2 && id) return { name: 'terminal', hostId: id };
      break;
  }
  return { name: 'not_found', path };
}

export function href(route: Route): string {
  switch (route.name) {
    case 'login': return '#/login';
    case 'hosts': return '#/hosts';
    case 'host': return `#/hosts/${encodeURIComponent(route.id)}`;
    case 'vms': return '#/vms';
    case 'vm': return `#/vms/${encodeURIComponent(route.id)}`;
    case 'console': return `#/vms/${encodeURIComponent(route.id)}/console`;
    case 'activity': return '#/activity';
    case 'terminal': return route.hostId ? `#/terminal/${encodeURIComponent(route.hostId)}` : '#/terminal';
    case 'not_found': return '#/hosts';
  }
}

export function navigate(route: Route): void {
  window.location.hash = href(route);
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseRoute(window.location.hash));
  useEffect(() => {
    const onChange = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  return route;
}
