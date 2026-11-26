import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { isApiError, type Host, type Vm } from '../api/types';
import { useApi, useSession } from './context';
import { jumpItems } from './derive';
import { Icon } from './icons';
import { href, navigate } from './router';

type Data = { status: 'idle' | 'loading' } | { status: 'ready'; hosts: Host[]; vms: Vm[] } | { status: 'error'; message: string };

/**
 * Top-bar search: a combobox whose jump list filters hosts, VMs and tools on
 * the client. The host and VM lists load once, on first focus, through the
 * same API the views use; nothing is sent per keystroke. `/` focuses it.
 */
export function Search() {
  const api = useApi();
  const { expire } = useSession();
  const [data, setData] = useState<Data>({ status: 'idle' });
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const input = useRef<HTMLInputElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const listId = useId();

  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (event.key !== '/' || event.ctrlKey || event.metaKey || event.altKey) return;
      if (target && (/^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) || target.isContentEditable)) return;
      event.preventDefault();
      input.current?.focus();
    };
    const onPointer = (event: PointerEvent) => {
      if (wrap.current && !wrap.current.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('pointerdown', onPointer);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onPointer);
    };
  }, []);

  function load() {
    if (data.status === 'loading' || data.status === 'ready') return;
    setData({ status: 'loading' });
    Promise.all([api.listHosts(), api.listVms()]).then(
      ([hosts, vms]) => setData({ status: 'ready', hosts, vms }),
      (error: unknown) => {
        if (isApiError(error, 'unauthorized')) expire();
        setData({ status: 'error', message: isApiError(error) ? error.message : 'Search is unavailable.' });
      },
    );
  }

  const items = data.status === 'ready' ? jumpItems(query, data.hosts, data.vms) : [];
  const expanded = open && query.trim() !== '';

  function close() {
    setOpen(false);
    setActive(-1);
  }

  function go(index: number) {
    const item = items[index];
    if (!item) return;
    navigate(item.route);
    setQuery('');
    close();
    input.current?.blur();
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      if (items.length === 0) return;
      event.preventDefault();
      setOpen(true);
      const step = event.key === 'ArrowDown' ? 1 : -1;
      setActive(i => (i + step + items.length) % items.length);
    } else if (event.key === 'Enter') {
      if (items.length === 0) return;
      event.preventDefault();
      go(Math.max(0, active));
    } else if (event.key === 'Escape') {
      if (expanded) {
        event.preventDefault();
        close();
      } else {
        setQuery('');
        input.current?.blur();
      }
    }
  }

  const options = expanded && data.status === 'ready' ? items : [];
  let message: string | null = null;
  if (expanded && options.length === 0) {
    message = data.status === 'ready' ? `No host, VM or tool matches “${query.trim()}”` : data.status === 'error' ? data.message : 'Loading hosts and VMs…';
  }

  return (
    <div className="search" ref={wrap}>
      <Icon name="search" />
      <input
        ref={input}
        type="search"
        role="combobox"
        aria-label="Search hosts, VMs and tools"
        aria-expanded={expanded}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={expanded && active >= 0 && items[active] ? `${listId}-${active}` : undefined}
        placeholder="Search hosts, VMs, tools"
        autoComplete="off"
        spellCheck={false}
        maxLength={128}
        value={query}
        onFocus={() => { load(); setOpen(true); }}
        onChange={e => { setQuery(e.target.value); setActive(-1); setOpen(true); }}
        onKeyDown={onKeyDown}
      />
      <kbd aria-hidden="true">/</kbd>
      <div className="jump" hidden={!expanded}>
        <div id={listId} role="listbox" aria-label="Search results">
          {options.map((item, i) => (
            <a key={item.key} id={`${listId}-${i}`} role="option" aria-selected={i === active} href={href(item.route)}
              onClick={e => { e.preventDefault(); go(i); }} onPointerEnter={() => setActive(i)}>
              <span className="jump-label">{item.label}</span>
              <small>{item.kind}</small>
            </a>
          ))}
        </div>
        {message ? <p role="status">{message}</p> : null}
      </div>
    </div>
  );
}
