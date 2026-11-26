import { useEffect, useRef, useState, type ReactNode } from 'react';
import { isApiError, type ConsoleStatus, type Host, type Vm } from '../../api/types';
import { useApi, useSession } from '../context';
import { consoleClosedReason } from '../derive';
import { formatDate } from '../format';
import { Icon } from '../icons';
import { href } from '../router';
import { ConsoleSessionStatus } from '../ui';

// The VM console tile: view-only frames (1–2 fps screendumps), states
// connecting / streaming / closed / error. Interactive VNC is later. A failed
// or stopped VM, or one on an offline host, opens straight to "closed" with
// the reason and never asks the server for a stream.


interface FrameView {
  url: string;
  seq: number;
  width: number;
  height: number;
  mime: string;
  capturedAt: string;
}

function Viewer({ vm, title }: { vm: Vm; title: ReactNode }) {
  const api = useApi();
  const { expire } = useSession();
  const [status, setStatus] = useState<ConsoleStatus>('connecting');
  const [reason, setReason] = useState<string | null>(null);
  const [frame, setFrame] = useState<FrameView | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [paused, setPaused] = useState(false);
  const urlRef = useRef<string | null>(null);

  useEffect(() => {
    if (paused) return;
    let live = true;
    let close: (() => void) | null = null;
    setStatus('connecting');
    setReason(null);
    api.openConsole(vm.id).then(
      stream => {
        if (!live) return stream.close();
        close = () => stream.close();
        setStatus(stream.status);
        stream.onStatus((s, why, sessionEnded) => {
          setStatus(s);
          setReason(why ?? null);
          if (sessionEnded) expire();
        });
        stream.onFrame(f => {
          const url = URL.createObjectURL(f.data);
          if (urlRef.current) URL.revokeObjectURL(urlRef.current);
          urlRef.current = url;
          setFrame({ url, seq: f.seq, width: f.width, height: f.height, mime: f.mime, capturedAt: f.capturedAt });
        });
      },
      (error: unknown) => {
        if (!live) return;
        if (isApiError(error, 'unauthorized')) expire();
        setStatus('error');
        setReason(isApiError(error) ? error.message : 'Could not open the console.');
      },
    );
    return () => {
      live = false;
      close?.();
    };
  }, [api, vm.id, attempt, paused, expire]);

  useEffect(() => () => {
    if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    urlRef.current = null;
  }, []);

  const label = `Console of ${vm.name}${frame ? `, frame ${frame.seq}` : ''}`;
  const shown = paused ? 'paused' : status;
  return (
    <>
      <div className="console-bar">
        {title}
        <ConsoleSessionStatus status={shown} />
        <span className="spacer" />
        <button type="button" className="button button-quiet" onClick={() => setPaused(p => !p)} aria-pressed={paused}>
          <Icon name={paused ? 'play' : 'pause'} />{paused ? 'Resume' : 'Pause'}
        </button>
        {!paused && (status === 'closed' || status === 'error')
          ? <button type="button" className="button" onClick={() => setAttempt(a => a + 1)}><Icon name="spin" />Reconnect</button>
          : null}
      </div>
      {reason ? <p className={status === 'error' ? 'form-error' : 'notice'} role={status === 'error' ? 'alert' : 'status'}>{reason}</p> : null}
      <div className="console-screen" data-status={status}>
        {frame
          ? <img src={frame.url} width={frame.width} height={frame.height} alt={label} />
          : <p className="console-placeholder" role="status">{status === 'connecting' ? 'Connecting to the console…' : 'No picture yet.'}</p>}
      </div>
      <p className="console-meta" aria-live="off">
        {frame ? <><span>seq <b>{frame.seq}</b></span><span>captured <b>{formatDate(frame.capturedAt)}</b></span><span>{frame.width}×{frame.height}</span><span>{frame.mime}</span></> : null}
        <span className="console-lock"><Icon name="lock" /><span className="console-mode">View only</span> · interactive VNC later</span>
      </p>
    </>
  );
}

/**
 * The console tile. `live` streams (the console route); otherwise the tile
 * waits for "Open console" so a visit to the VM never opens a stream by itself.
 * `onRecheck` reloads the VM and host so a closed console can come back.
 */
export function ConsoleTile({ vm, host, live, onRecheck }: { vm: Vm; host: Host | undefined; live: boolean; onRecheck(): void }) {
  const closed = consoleClosedReason(vm, host);
  const title = <h2 id="console-title"><Icon name="monitor" />Console</h2>;
  let body;
  if (closed !== null) {
    body = (
      <>
        <div className="console-bar">
          {title}
          <ConsoleSessionStatus status="closed" />
          <span className="spacer" />
          <button type="button" className="button" onClick={onRecheck}><Icon name="spin" />Reconnect</button>
        </div>
        <div className="console-screen" data-status="closed">
          <p className="console-placeholder console-closed" role="status"><Icon name={host?.reachability === 'offline' ? 'off' : vm.state === 'failed' ? 'fail' : 'stop'} />{closed}</p>
        </div>
        <p className="console-meta"><span className="console-lock"><Icon name="lock" /><span className="console-mode">View only</span> · interactive VNC later</span></p>
      </>
    );
  } else if (live) {
    body = <Viewer vm={vm} title={title} />;
  } else {
    body = (
      <>
      <div className="console-bar">{title}</div>
      <div className="console-screen console-idle">
        <div className="console-placeholder">
          <Icon name="monitor" />
          <p>View-only frames, one or two a second.</p>
          <a className="button button-primary" href={href({ name: 'console', id: vm.id })}><Icon name="play" />Open console</a>
        </div>
      </div>
      </>
    );
  }
  return (
    <section id="console" className="tile s8 console-tile" aria-labelledby="console-title">
      {body}
    </section>
  );
}
