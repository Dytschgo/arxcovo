import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { ApiError, isApiError, type ArxApi, type Credentials, type Session } from '../api/types';

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
  login(credentials: Credentials): Promise<void>;
  logout(): Promise<void>;
  /** Called when any request reports the session is gone. */
  expire(): void;
}

const SessionContext = createContext<SessionValue | null>(null);

export function useSession(): SessionValue {
  const value = useContext(SessionContext);
  if (!value) throw new Error('useSession outside AppProviders');
  return value;
}

export function AppProviders({ api, children }: { api: ArxApi; children: ReactNode }) {
  const [state, setState] = useState<SessionState>({ status: 'checking' });

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
    setState({ status: 'authenticated', session });
  }, [api]);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      setState({ status: 'anonymous', notice: 'You are signed out.' });
    }
  }, [api]);

  const expire = useCallback(() => {
    setState(s => (s.status === 'authenticated' ? { status: 'anonymous', notice: 'Your session has ended. Sign in again.' } : s));
  }, []);

  const value = useMemo(() => ({ state, login, logout, expire }), [state, login, logout, expire]);
  return (
    <ApiContext.Provider value={api}>
      <SessionContext.Provider value={value}>{children}</SessionContext.Provider>
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
