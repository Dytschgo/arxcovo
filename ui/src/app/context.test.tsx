import { act, render } from '@testing-library/react';
import { describe, expect, test } from 'vitest';
import { DEMO_CREDENTIALS, FIXTURE_HOSTS, FIXTURE_VMS } from '../api/fixtures';
import { StandinApi } from '../api/standin';
import { AppProviders, INVENTORY_TTL_MS, useInventory, useSession, type Inventory } from './context';

// The session-scoped inventory behind the search jump list: results are tied
// to the session that loaded them and never outlive it.
function harness() {
  const seen: { inventory?: Inventory; session?: ReturnType<typeof useSession> } = {};
  function Probe() {
    seen.inventory = useInventory();
    seen.session = useSession();
    return null;
  }
  render(<AppProviders api={new StandinApi()}><Probe /></AppProviders>);
  return seen as Required<typeof seen>;
}

describe('inventory', () => {
  test('signed out there is no epoch and nothing is kept', async () => {
    const s = harness();
    await act(async () => {});
    expect(s.inventory.epoch()).toBeNull();
    s.inventory.put(null, { hosts: FIXTURE_HOSTS });
    expect(s.inventory.fresh()).toEqual({});
  });

  test('lists loaded in this session are reused while fresh, then expire', async () => {
    const s = harness();
    await act(async () => s.session.login(DEMO_CREDENTIALS));
    const epoch = s.inventory.epoch();
    expect(epoch).not.toBeNull();
    s.inventory.put(epoch, { hosts: FIXTURE_HOSTS }, 1_000);
    s.inventory.put(epoch, { vms: FIXTURE_VMS }, 1_000);
    expect(s.inventory.fresh(1_000 + INVENTORY_TTL_MS)).toEqual({ hosts: FIXTURE_HOSTS, vms: FIXTURE_VMS });
    expect(s.inventory.fresh(1_001 + INVENTORY_TTL_MS)).toEqual({ hosts: undefined, vms: undefined });
  });

  test('a result from an earlier session is dropped, and a new session starts empty', async () => {
    const s = harness();
    await act(async () => s.session.login(DEMO_CREDENTIALS));
    const first = s.inventory.epoch();
    s.inventory.put(first, { hosts: FIXTURE_HOSTS });
    await act(async () => { await s.session.logout(); });
    expect(s.inventory.epoch()).toBeNull();
    expect(s.inventory.fresh()).toEqual({});
    await act(async () => s.session.login(DEMO_CREDENTIALS));
    const second = s.inventory.epoch();
    expect(second).not.toBe(first);
    expect(s.inventory.fresh()).toEqual({});
    s.inventory.put(first, { vms: FIXTURE_VMS });
    expect(s.inventory.fresh()).toEqual({});
    s.inventory.put(second, { hosts: FIXTURE_HOSTS });
    // A late result from the first session neither shows nor wipes the second's lists.
    s.inventory.put(first, { vms: FIXTURE_VMS });
    expect(s.inventory.fresh()).toEqual({ hosts: FIXTURE_HOSTS, vms: undefined });
    s.inventory.put(second, { vms: FIXTURE_VMS });
    expect(s.inventory.fresh().vms).toBe(FIXTURE_VMS);
  });

  test('an ended session drops what it loaded', async () => {
    const s = harness();
    await act(async () => s.session.login(DEMO_CREDENTIALS));
    s.inventory.put(s.inventory.epoch(), { hosts: FIXTURE_HOSTS });
    act(() => s.session.expire());
    expect(s.inventory.epoch()).toBeNull();
    expect(s.inventory.fresh()).toEqual({});
  });
});
