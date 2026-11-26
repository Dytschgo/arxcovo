import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { ApiError, isApiError, type ArxApi, type Credentials, type Host, type Session, type Vm } from '../api/types';

const ApiContext = createContext<ArxApi | null>(null);

export function useApi(): ArxApi {
  const api = useContext(ApiContext);
  if (!api) throw new Error('useApi outside ApiProvider');
  return api;
}

export type SessionState =
  | { status: 'checking' }
  | { status: 'anonymous'; notice?: string; error?: string }
  | { status: 'authenticated'; session: Session };

interface SessionValue {
  state: SessionState;
  /** Set when the last sign-out attempt failed; the session is still active. */
  logoutError: string | null;
  login(credentials: Credentials): Promise<void>;
  /** Resolves true once the server confirmed the sign-out. */
  logout(): Promise<boolean>;
  /** Called when any request reports the session is gone. */
  expire(): void;
}

const SessionContext = createContext<SessionValue | null>(null);

/** How long a loaded host or VM list counts as fresh for reuse (search jump list). */
export const INVENTORY_TTL_MS = 60_000;

/**
 * The host and VM lists the views already loaded in this session, so the
 * search jump list can reuse them instead of asking again. Every entry is
 * tied to the session that loaded it: `epoch()` names the current session,
 * `put` drops results from an older one, and a new sign-in starts empty.
 */
export interface Inventory {
  /** The current session's identity, or null when signed out. Capture it before a request. */
  epoch(): object | null;
  /** Lists loaded in the current session within the TTL. */
  fresh(now?: number): { hosts?: Host[]; vms?: Vm[] };
  /** Records lists loaded under `epoch`; ignored when that session is gone. */
  put(epoch: object | null, lists: { hosts?: Host[]; vms?: Vm[] }, now?: number): void;
}

const InventoryContext = createContext<Inventory | null>(null);

export function useInventory(): Inventory {
  const value = useContext(InventoryContext);
  if (!value) throw new Error('useInventory outside AppProviders');
  return value;
}

export function useSession(): SessionValue {
  const value = useContext(SessionContext);
  if (!value) throw new Error('useSession outside AppProviders');
  return value;
}

export function AppProviders({ api, children }: { api: ArxApi; children: ReactNode }) {
  const [state, setState] = useState<SessionState>({ status: 'checking' });
  const [logoutError, setLogoutError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api.getSession().then(
      session => live && setState(session ? { status: 'authenticated', session } : { status: 'anonymous' }),
      (error: unknown) => live && setState({ status: 'anonymous', error: isApiError(error) ? error.message : 'Cannot reach arxd.' }),
    );
    return () => {
      live = false;
    };
  }, [api]);

  const login = useCallback(async (credentials: Credentials) => {
    const session = await api.login(credentials);
    setLogoutError(null);
    setState({ status: 'authenticated', session });
  }, [api]);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } catch (error) {
      // The server did not confirm the revocation: the cookie may still be
      // valid, so keep the session visible and let the owner retry.
      setLogoutError(isApiError(error) ? error.message : 'Sign-out failed.');
      return false;
    }
    setLogoutError(null);
    setState({ status: 'anonymous', notice: 'You are signed out.' });
    return true;
  }, [api]);

  const expire = useCallback(() => {
    setLogoutError(null);
    setState(s => (s.status === 'authenticated' ? { status: 'anonymous', notice: 'Your session has ended. Sign in again.' } : s));
  }, []);

  const value = useMemo(() => ({ state, logoutError, login, logout, expire }), [state, logoutError, login, logout, expire]);

  // The session object is the epoch: a new sign-in is a new object, signed out is null.
  const epochRef = useRef<object | null>(null);
  const lists = useRef<{ epoch: object | null; hosts?: { data: Host[]; at: number }; vms?: { data: Vm[]; at: number } }>({ epoch: null });
  epochRef.current = state.status === 'authenticated' ? state.session : null;
  const inventory = useMemo<Inventory>(() => ({
    epoch: () => epochRef.current,
    fresh: (now = Date.now()) => {
      const l = lists.current;
      if (l.epoch === null || l.epoch !== epochRef.current) return {};
      const ok = (e?: { at: number }) => e !== undefined && now - e.at <= INVENTORY_TTL_MS;
      return { hosts: ok(l.hosts) ? l.hosts!.data : undefined, vms: ok(l.vms) ? l.vms!.data : undefined };
    },
    put: (epoch, { hosts, vms }, now = Date.now()) => {
      if (epoch === null || epoch !== epochRef.current) return;
      if (lists.current.epoch !== epoch) lists.current = { epoch };
      if (hosts) lists.current.hosts = { data: hosts, at: now };
      if (vms) lists.current.vms = { data: vms, at: now };
    },
  }), []);

  return (
    <ApiContext.Provider value={api}>
      <SessionContext.Provider value={value}>
        <InventoryContext.Provider value={inventory}>{children}</InventoryContext.Provider>
      </SessionContext.Provider>
    </ApiContext.Provider>
  );
}

export type Resource<T> =
  | { status: 'loading'; data?: undefined; error?: undefined }
  | { status: 'ready'; data: T; error?: undefined }
  | { status: 'error'; data?: undefined; error: ApiError };

/**
 * Loads data through the API; an `unauthorized` failure ends the session so
 * the login view replaces the current one.
 */
export function useResource<T>(load: (api: ArxApi) => Promise<T>, deps: readonly unknown[]): Resource<T> & { reload(): void } {
  const api = useApi();
  const { expire } = useSession();
  const [state, setState] = useState<Resource<T>>({ status: 'loading' });
  const [attempt, setAttempt] = useState(0);
  const loadRef = useRef(load);
  loadRef.current = load;

  useEffect(() => {
    let live = true;
    setState({ status: 'loading' });
    loadRef.current(api).then(
      data => live && setState({ status: 'ready', data }),
      (error: unknown) => {
        if (!live) return;
        const e = isApiError(error) ? error : new ApiError('bad_response', 'Something went wrong while loading.');
        if (e.code === 'unauthorized') expire();
        setState({ status: 'error', error: e });
      },
    );
    return () => {
      live = false;
    };
  }, [api, expire, attempt, ...deps]);

  const reload = useCallback(() => setAttempt(a => a + 1), []);
  return { ...state, reload };
}
