import { useEffect, useRef, useState } from 'react';
import { isApiError, type ConsoleStatus, type Vm } from '../../api/types';
import { useApi, useResource, useSession } from '../context';
import { formatDate } from '../format';
import { href } from '../router';
import { ArchBadge, Loaded, Page, Pill, VmStatus, type Tone } from '../ui';

const TONE: Record<ConsoleStatus, Tone> = { connecting: 'info', streaming: 'ok', closed: 'muted', error: 'bad' };

interface FrameView {
  url: string;
  seq: number;
  width: number;
  height: number;
  capturedAt: string;
}

function Viewer({ vm }: { vm: Vm }) {
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
          setFrame({ url, seq: f.seq, width: f.width, height: f.height, capturedAt: f.capturedAt });
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
  return (
    <div className="console">
      <div className="terminal-status">
        <Pill tone={paused ? 'muted' : TONE[status]}><span className="visually-hidden">Console status: </span>{paused ? 'paused' : status}</Pill>
        <VmStatus state={vm.state} />
        <ArchBadge arch={vm.arch} />
        <span className="console-mode">View only</span>
        <span className="spacer" />
        <button type="button" className="button button-quiet" onClick={() => setPaused(p => !p)} aria-pressed={paused}>{paused ? 'Resume' : 'Pause'}</button>
        {!paused && (status === 'closed' || status === 'error') ? <button type="button" className="button" onClick={() => setAttempt(a => a + 1)}>Reconnect</button> : null}
      </div>
      {reason ? <p className={status === 'error' ? 'form-error' : 'notice'} role={status === 'error' ? 'alert' : 'status'}>{reason}</p> : null}
      <div className="console-screen" data-status={status}>
        {frame ? (
          <img src={frame.url} width={frame.width} height={frame.height} alt={label} />
        ) : (
          <p className="console-placeholder" role="status">{status === 'connecting' ? 'Connecting to the console…' : 'No picture yet.'}</p>
        )}
      </div>
      <p className="console-meta" aria-live="off">
        {frame ? <>Frame {frame.seq} · {frame.width}×{frame.height} · captured {formatDate(frame.capturedAt)}</> : null}
      </p>
    </div>
  );
}

export function ConsoleView({ id }: { id: string }) {
  const vm = useResource(api => api.getVm(id), [id]);
  return (
    <Page title={vm.status === 'ready' ? `Console · ${vm.data.name}` : 'Console'} eyebrow={<a href={href({ name: 'vm', id })}>{vm.status === 'ready' ? vm.data.name : id}</a>}>
      <Loaded resource={vm} label="this VM">
        {data => <Viewer vm={data} />}
      </Loaded>
    </Page>
  );
}
