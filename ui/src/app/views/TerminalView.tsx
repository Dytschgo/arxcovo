import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { exampleCommand, parseCommandLine } from '../../api/command';
import { isApiError, type Host, type TerminalSession, type TerminalStatus } from '../../api/types';
import { useApi, useResource, useSession } from '../context';
import { Icon } from '../icons';
import { href } from '../router';
import { ArchBadge, Empty, HostStatus, Loaded, OsBadge, Page, TerminalSessionStatus } from '../ui';

export const MAX_LINES = 2000;

type Line = { id: number; kind: 'cmd' | 'stdout' | 'stderr' | 'exit' | 'error' | 'info'; text: string };

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
            case 'closed':
              append('error', event.message);
              setConnectError(event.message);
              if (event.reason === 'session_ended') expire();
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
    <section className="tile term-tile" aria-labelledby="term-title">
      <header className="term-head">
        <h2 id="term-title" className="term-host"><OsBadge os={host.os} /><span>{host.name}</span></h2>
        <ArchBadge arch={host.arch} />
        <TerminalSessionStatus status={status} />
        {status === 'closed' || status === 'error' ? <button type="button" className="button button-quiet" onClick={() => setAttempt(a => a + 1)}><Icon name="spin" />Reconnect</button> : null}
        <span className="spacer" />
        <span className="term-legend" aria-hidden="true"><span>› stdout</span><span>! stderr</span></span>
      </header>
      {connectError ? <p className="form-error" role="alert">{connectError}</p> : null}
      <div className="terminal" ref={outputRef} role="log" aria-live="polite" aria-label={`Terminal output for ${host.name}`} tabIndex={0}>
        {lines.length === 0 ? (
          <div className="terminal-empty">
            <Icon name="term" />
            <p>{status === 'connecting' ? 'Connecting…' : 'Pick a tool, add arguments, press Enter. Commands run without a shell, with bounded time and output.'}</p>
          </div>
        ) : null}
        {lines.map(l => (
          <div key={l.id} className="terminal-line" data-kind={l.kind}>
            {l.kind === 'cmd' ? <span className="terminal-prompt" aria-hidden="true">$ </span> : null}
            {l.kind === 'cmd' ? <span className="visually-hidden">Command: </span> : null}
            {l.kind === 'stderr' ? <span className="terminal-prompt" aria-hidden="true">! </span> : null}
            {l.text}
          </div>
        ))}
      </div>
      <div className="term-form">
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
            placeholder={exampleCommand(host.tools)}
            aria-describedby="terminal-help"
            aria-invalid={inputError ? true : undefined}
            disabled={status !== 'ready' && status !== 'running'}
          />
          {status === 'running'
            ? <button type="button" className="button" onClick={() => session.current?.cancel()}><Icon name="stop" />Cancel</button>
            : <button type="submit" className="button button-primary" disabled={status !== 'ready'}><Icon name="play" />Run</button>}
        </form>
        {inputError ? <p className="form-error" role="alert">{inputError}</p> : null}
        <div id="terminal-help" className="terminal-help">
          <span>Tools on {host.name}:</span>
          <ul className="chips">
            {host.tools.map(t => (
              <li key={t.name}>
                <button type="button" className="chip chip-button" onClick={() => { setInput(`${t.name} --version`); inputRef.current?.focus(); }}>
                  <code>{t.name}</code>{t.version ? <span className="chip-version">{t.version}</span> : null}
                </button>
              </li>
            ))}
          </ul>
        </div>
        <p className="term-rules">
          <span><Icon name="run" />One command at a time</span>
          <span><Icon name="monitor" />Output bounded</span>
          <span><Icon name="shield" />Same policy and limits as agents over MCP</span>
          <span><Icon name="act" />Logged to Activity as owner</span>
          <span className="terminal-keys"><kbd>Enter</kbd> run · <kbd>↑</kbd><kbd>↓</kbd> history · <kbd>Ctrl</kbd>+<kbd>C</kbd> cancel</span>
        </p>
      </div>
    </section>
  );
}

function HostPicker({ hosts, selected }: { hosts: Host[]; selected: string }) {
  const reachable = hosts.filter(h => h.reachability !== 'offline').length;
  return (
    <section className="tile term-hosts" aria-labelledby="term-hosts">
      <header className="tile-head">
        <h2 id="term-hosts"><Icon name="hosts" />Host</h2>
        <span className="tile-aside">{reachable} of {hosts.length} reachable</span>
      </header>
      <ul className="host-pick" aria-labelledby="term-hosts">
        {hosts.map(h => {
          const body = (
            <>
              <span className="hp-top"><OsBadge os={h.os} /><b className="mono">{h.name}</b><HostStatus reachability={h.reachability} /></span>
              <span className="hp-sub">{h.osVersion} <ArchBadge arch={h.arch} /></span>
              {h.reachabilityReason ? <span className="hp-why">{h.reachabilityReason}</span> : null}
            </>
          );
          return (
            <li key={h.id} data-reachability={h.reachability}>
              {h.reachability === 'offline'
                ? <span className="hp" aria-disabled="true">{body}</span>
                : <a className="hp" href={href({ name: 'terminal', hostId: h.id })} aria-current={h.id === selected ? 'true' : undefined}>{body}</a>}
            </li>
          );
        })}
      </ul>
      <p className="note"><Icon name="lock" />Tools are the ones the host discovered. Offline hosts answer 503.</p>
    </section>
  );
}

export function TerminalView({ hostId }: { hostId: string | null }) {
  const hosts = useResource(api => api.listHosts(), []);
  return (
    <Page title="Terminal" lede="Run a tool on a host · not a shell" actions={<span className="tag"><Icon name="shield" />run_command</span>}>
      <Loaded resource={hosts} label="hosts">
        {list => {
          const usable = list.filter(h => h.reachability !== 'offline');
          if (usable.length === 0) return <Empty title="No reachable hosts">A terminal needs at least one online host.</Empty>;
          const selected = list.find(h => h.id === hostId) ?? usable[0];
          return (
            <div className="term-grid">
              <HostPicker hosts={list} selected={selected.id} />
              {selected.reachability === 'offline'
                ? <div className="tile"><Empty title={`${selected.name} is offline`}>{selected.reachabilityReason ?? 'Choose another host.'}</Empty></div>
                : <Terminal key={selected.id} host={selected} />}
            </div>
          );
        }}
      </Loaded>
    </Page>
  );
}
