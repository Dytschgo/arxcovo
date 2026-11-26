import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { isApiError, type Host, type Vm } from '../api/types';
import { INVENTORY_TTL_MS, useApi, useInventory, useSession } from './context';
import { jumpItems } from './derive';
import { Icon } from './icons';
import { href, navigate } from './router';

type Data = { status: 'idle' | 'loading' } | { status: 'ready'; hosts: Host[]; vms: Vm[] } | { status: 'error'; message: string };

/**
 * Top-bar search: an ARIA combobox (focus stays in the input; options are
 * reached with the arrows through aria-activedescendant) whose jump list
 * filters hosts, VMs and tools on the client.
 *
 * On focus it reuses the host and VM lists a view already loaded in this
 * session (the inventory); only a missing or stale list is requested, and
 * nothing is sent per keystroke. A result or a 401 that arrives after the
 * session changed, or after the search unmounted, is ignored, so a late
 * answer from an old session can never sign the owner out of a new one.
 */
export function Search() {
  const api = useApi();
  const { expire } = useSession();
  const inventory = useInventory();
  const mounted = useRef(true);
  const [data, setData] = useState<Data>({ status: 'idle' });
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const input = useRef<HTMLInputElement>(null);
  const loadedAt = useRef(0);
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
    mounted.current = true;
    return () => {
      mounted.current = false;
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onPointer);
    };
  }, []);

  function load() {
    if (data.status === 'loading') return;
    const cached = inventory.fresh();
    if (cached.hosts && cached.vms) {
      setData({ status: 'ready', hosts: cached.hosts, vms: cached.vms });
      return;
    }
    if (data.status === 'ready' && Date.now() - loadedAt.current <= INVENTORY_TTL_MS) return;
    const epoch = inventory.epoch();
    const current = () => mounted.current && epoch !== null && inventory.epoch() === epoch;
    setData({ status: 'loading' });
    Promise.all([cached.hosts ?? api.listHosts(), cached.vms ?? api.listVms()]).then(
      ([hosts, vms]) => {
        if (!current()) return;
        inventory.put(epoch, { hosts, vms });
        loadedAt.current = Date.now();
        setData({ status: 'ready', hosts, vms });
      },
      (error: unknown) => {
        if (!current()) return;
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
      const step = event.key === 'ArrowDown' ? 1 : -1;
      // Opening with ArrowUp lands on the last option, ArrowDown on the first.
      setActive(i => (!expanded || i < 0 ? (step > 0 ? 0 : items.length - 1) : (i + step + items.length) % items.length));
      setOpen(true);
    } else if (event.key === 'Enter') {
      // Enter acts only on an open list; a hidden result is never activated.
      if (!expanded || items.length === 0) return;
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
        onBlur={close}
        onChange={e => { setQuery(e.target.value); setActive(-1); setOpen(true); }}
        onKeyDown={onKeyDown}
      />
      <kbd aria-hidden="true">/</kbd>
      <div className="jump" hidden={!expanded}>
        <div id={listId} role="listbox" aria-label="Search results">
          {options.map((item, i) => (
            <a key={item.key} id={`${listId}-${i}`} role="option" aria-selected={i === active} href={href(item.route)} tabIndex={-1}
              onMouseDown={e => e.preventDefault()} onClick={e => { e.preventDefault(); go(i); }} onPointerEnter={() => setActive(i)}>
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
