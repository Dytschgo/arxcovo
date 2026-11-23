import { useEffect, useRef, useState, type FormEvent } from 'react';
import { isApiError } from '../../api/types';
import { useApi, useSession } from '../context';

export function LoginView({ notice, error: initialError }: { notice?: string; error?: string }) {
  const api = useApi();
  const { login } = useSession();
  const [username, setUsername] = useState('owner');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(initialError ?? null);
  const [busy, setBusy] = useState(false);
  const passwordRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    document.title = 'Sign in · Arxcovo';
    passwordRef.current?.focus();
  }, []);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    if (!username || !password) {
      setError('Enter the username and password.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await login({ username, password });
    } catch (e) {
      setError(isApiError(e) ? e.message : 'Sign-in failed.');
      setPassword('');
      passwordRef.current?.focus();
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="login">
      <form className="login-card" onSubmit={submit} noValidate aria-labelledby="login-title" aria-describedby={error ? 'login-error' : undefined}>
        <div className="brand brand-large" aria-hidden="true"><span className="brand-mark" />Arxcovo</div>
        <h1 id="login-title" className="visually-hidden">Arxcovo</h1>
        <p className="login-lede">Sign in to manage this cluster.</p>
        {notice && !error ? <p className="notice" role="status">{notice}</p> : null}
        {error ? <p id="login-error" className="form-error" role="alert">{error}</p> : null}
        <label className="field">
          <span>Username</span>
          <input name="username" autoComplete="username" value={username} onChange={e => setUsername(e.target.value)} required maxLength={64} spellCheck={false} autoCapitalize="none" />
        </label>
        <label className="field">
          <span>Password</span>
          <input ref={passwordRef} name="password" type="password" autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)} required maxLength={1024} aria-invalid={error ? true : undefined} />
        </label>
        <button className="button button-primary" type="submit" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</button>
        {/* __ARX_STANDIN__ is false for every `vite build`, so bundles never contain the demo password. */}
        {__ARX_STANDIN__ && api.kind === 'standin' ? (
          <p className="hint">Demo data: sign in as <code>owner</code> with password <code>arxcovo-demo</code>.</p>
        ) : (
          <p className="hint">The session is kept in a secure, HTTP-only cookie on this host.</p>
        )}
      </form>
    </div>
  );
}
