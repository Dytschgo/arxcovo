import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { parseCommandLine } from '../../api/command';
import { isApiError, type Host, type TerminalSession, type TerminalStatus } from '../../api/types';
import { useApi, useResource, useSession } from '../context';
import { href, navigate } from '../router';
import { ArchBadge, Empty, Loaded, Page, Pill, SelectField, type Tone } from '../ui';

export const MAX_LINES = 2000;

type Line = { id: number; kind: 'cmd' | 'stdout' | 'stderr' | 'exit' | 'error' | 'info'; text: string };

const STATUS_TONE: Record<TerminalStatus, Tone> = { connecting: 'info', ready: 'ok', running: 'info', closed: 'muted', error: 'bad' };

function Terminal({ host }: { host: Host }) {
  const api = useApi();
  const { expire } = useSession();
  const [status, setStatus] = useState<TerminalStatus>('connecting');
  const [lines, setLines] = useState<Line[]>([]);
  const [input, setInput] = useState('');
  const [inputError, setInputError] = useState<string | null>(null);
  const [connectError, setConnectError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [history, setHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState<number | null>(null);
  const session = useRef<TerminalSession | null>(null);
  const nextId = useRef(0);
  const outputRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let live = true;
    let current: TerminalSession | null = null;
    const append = (kind: Line['kind'], text: string) => setLines(ls => [...ls, { id: nextId.current++, kind, text }].slice(-MAX_LINES));
    setStatus('connecting');
    setConnectError(null);
    setLines([]);
    api.openTerminal(host.id).then(
      s => {
        if (!live) return s.close();
        current = s;
        session.current = s;
        setStatus(s.status);
        s.onEvent(event => {
          switch (event.type) {
            case 'status':
              setStatus(event.status);
              if (event.status === 'ready') inputRef.current?.focus();
              break;
            case 'started':
              append('cmd', [event.tool, ...event.args].join(' '));
              break;
            case 'output':
              append(event.stream, event.data);
              break;
            case 'exit':
              append('exit', `exit ${event.exitCode ?? '—'} · ${event.outcome}${event.truncated ? ' · output truncated' : ''}`);
              break;
            case 'error':
              append('error', event.message);
              break;
          }
        });
      },
      (error: unknown) => {
        if (!live) return;
        if (isApiError(error, 'unauthorized')) expire();
        setStatus('error');
        setConnectError(isApiError(error) ? error.message : 'Could not open a terminal.');
      },
    );
    return () => {
      live = false;
      current?.close();
      session.current = null;
    };
  }, [api, host.id, attempt, expire]);

  useEffect(() => {
    const el = outputRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines]);

  function submit(event: FormEvent) {
    event.preventDefault();
    const parsed = parseCommandLine(input);
    if (!parsed.ok) {
      setInputError(parsed.error);
      return;
    }
    if (!session.current || session.current.status !== 'ready') {
      setInputError('Wait for the terminal to be ready.');
      return;
    }
    setInputError(null);
    try {
      session.current.run(parsed.command);
    } catch (e) {
      setInputError(isApiError(e) ? e.message : 'Could not run the command.');
      return;
    }
    setHistory(h => [...h.filter(x => x !== input), input].slice(-50));
    setHistoryIndex(null);
    setInput('');
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'ArrowUp' && history.length > 0) {
      event.preventDefault();
      const i = historyIndex === null ? history.length - 1 : Math.max(0, historyIndex - 1);
      setHistoryIndex(i);
      setInput(history[i]);
    } else if (event.key === 'ArrowDown' && historyIndex !== null) {
      event.preventDefault();
      const i = historyIndex + 1;
      if (i >= history.length) {
        setHistoryIndex(null);
        setInput('');
      } else {
        setHistoryIndex(i);
        setInput(history[i]);
      }
    } else if (event.key === 'c' && event.ctrlKey && status === 'running') {
      event.preventDefault();
      session.current?.cancel();
    }
  }

  return (
    <div className="terminal-wrap">
      <div className="terminal-status">
        <Pill tone={STATUS_TONE[status]}><span className="visually-hidden">Terminal status: </span>{status}</Pill>
        <span>{host.name}</span>
        <ArchBadge arch={host.arch} />
        {status === 'closed' || status === 'error' ? <button type="button" className="button button-quiet" onClick={() => setAttempt(a => a + 1)}>Reconnect</button> : null}
      </div>
      {connectError ? <p className="form-error" role="alert">{connectError}</p> : null}
      <div className="terminal" ref={outputRef} role="log" aria-live="polite" aria-label={`Terminal output for ${host.name}`} tabIndex={0}>
        {lines.length === 0 ? <p className="terminal-empty">{status === 'connecting' ? 'Connecting…' : 'Run one of the host’s tools. Commands run without a shell, with bounded time and output.'}</p> : null}
        {lines.map(l => (
          <div key={l.id} className="terminal-line" data-kind={l.kind}>
            {l.kind === 'cmd' ? <span className="terminal-prompt" aria-hidden="true">$ </span> : null}
            {l.kind === 'cmd' ? <span className="visually-hidden">Command: </span> : null}
            {l.text}
          </div>
        ))}
      </div>
      <form className="terminal-input" onSubmit={submit}>
        <label htmlFor="terminal-command" className="visually-hidden">Command</label>
        <span className="terminal-prompt" aria-hidden="true">$</span>
        <input
          id="terminal-command"
          ref={inputRef}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          autoComplete="off"
          spellCheck={false}
          autoCapitalize="none"
          maxLength={16384}
          placeholder="git --version"
          aria-describedby="terminal-help"
          aria-invalid={inputError ? true : undefined}
          disabled={status !== 'ready' && status !== 'running'}
        />
        {status === 'running'
          ? <button type="button" className="button" onClick={() => session.current?.cancel()}>Cancel</button>
          : <button type="submit" className="button button-primary" disabled={status !== 'ready'}>Run</button>}
      </form>
      {inputError ? <p className="form-error" role="alert">{inputError}</p> : null}
      <div id="terminal-help" className="terminal-help">
        <span>Tools on {host.name}:</span>
        <ul className="chips">
          {host.tools.map(t => (
            <li key={t.name}>
              <button type="button" className="chip chip-button" onClick={() => { setInput(`${t.name} --version`); inputRef.current?.focus(); }}>
                <code>{t.name}</code>
              </button>
            </li>
          ))}
        </ul>
        <span className="terminal-keys">↑/↓ history · Ctrl+C cancels a running command</span>
      </div>
    </div>
  );
}

export function TerminalView({ hostId }: { hostId: string | null }) {
  const hosts = useResource(api => api.listHosts(), []);
  return (
    <Page title="Terminal" eyebrow="Bounded commands on a host">
      <Loaded resource={hosts} label="hosts">
        {list => {
          const usable = list.filter(h => h.reachability !== 'offline');
          if (usable.length === 0) return <Empty title="No reachable hosts">A terminal needs at least one online host.</Empty>;
          const selected = list.find(h => h.id === hostId) ?? usable[0];
          return (
            <>
              <div className="toolbar">
                <SelectField label="Host" value={selected.id} onChange={v => navigate({ name: 'terminal', hostId: v })}>
                  {list.map(h => <option key={h.id} value={h.id} disabled={h.reachability === 'offline'}>{h.name}{h.reachability === 'offline' ? ' (offline)' : ''}</option>)}
                </SelectField>
                <a href={href({ name: 'host', id: selected.id })}>Host details</a>
              </div>
              {selected.reachability === 'offline'
                ? <Empty title={`${selected.name} is offline`}>{selected.reachabilityReason ?? 'Choose another host.'}</Empty>
                : <Terminal key={selected.id} host={selected} />}
            </>
          );
        }}
      </Loaded>
    </Page>
  );
}
