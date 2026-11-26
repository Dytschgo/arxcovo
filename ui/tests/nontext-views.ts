import type { Page } from '@playwright/test';
import { compositing, type Compositing, type Layer, type Rgba } from './compositing';
import type { NonTextFailure } from './nontext';

/**
 * Non-text contrast (WCAG 2.2 SC 1.4.11, 3:1) for the PR B view parts that
 * the shared audit (./nontext.ts) does not reach, with the same compositing:
 *
 * - toggle buttons (`aria-pressed`): a pressed one's fill against the group
 *   it sits in; an unpressed chip's border against the surface outside it;
 * - segmented groups (`.seg`): the group's border against the page around it;
 * - the switch (`.switch-track`): its edge (inset ring) against the page, and
 *   the knob against the track, off and on;
 * - capacity bars (`.cap-bar`, `.res-bar`): the fill against its track.
 * (Link underlines are covered by the shared audit.)
 */
export async function viewPartFailures(page: Page, minimum = 3): Promise<NonTextFailure[]> {
  return page.evaluate(`(${partsInPage.toString()})((${compositing.toString()})(), ${JSON.stringify(minimum)})`) as Promise<NonTextFailure[]>;
}

function partsInPage(C: Compositing, min: number): NonTextFailure[] {
  const describe = (el: Element) => el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).join('.') : '')
    + (el.textContent?.trim() ? `("${el.textContent.trim().slice(0, 30)}")` : '');
  const chain = (el: Element | null): Layer[] => {
    const out: Layer[] = [];
    for (let node = el; node; node = node.parentElement) {
      const s = getComputedStyle(node);
      out.push({ background: C.parse(s.backgroundColor), opacity: parseFloat(s.opacity) });
    }
    return out;
  };
  const clear: Rgba = [0, 0, 0, 0];
  const outside = (el: Element) => C.composite(clear, chain(el.parentElement)).background;
  const inside = (el: Element) => C.composite(clear, chain(el)).background;
  const paintOn = (colour: string, el: Element) => C.composite(C.parse(colour), chain(el)).ink;
  const painted = (el: Element) => {
    const r = el.getBoundingClientRect();
    if (r.width <= 1 || r.height <= 1) return false;
    const s = getComputedStyle(el);
    return s.visibility !== 'hidden' && s.display !== 'none';
  };
  const failures: NonTextFailure[] = [];
  const check = (el: Element, part: string, state: string, fg: Rgba, bg: Rgba) => {
    const raw = C.ratio(fg, bg);
    if (raw < min) failures.push({ what: describe(el), part, state, fg: C.hex(fg), bg: C.hex(bg), ratio: Math.floor(raw * 100) / 100 });
  };

  for (const el of document.body.querySelectorAll('[aria-pressed]')) {
    if (!painted(el)) continue;
    if (el.getAttribute('aria-pressed') === 'true') check(el, 'pressed fill against its group', 'pressed', inside(el), outside(el));
    else if (el.classList.contains('fchip')) check(el, 'chip border', 'rest', paintOn(getComputedStyle(el).borderTopColor, el), outside(el));
  }
  for (const el of document.body.querySelectorAll('.seg')) {
    if (painted(el)) check(el, 'segmented group border', 'rest', paintOn(getComputedStyle(el).borderTopColor, el), outside(el));
  }
  for (const track of document.body.querySelectorAll('.switch-track')) {
    if (!painted(track)) continue;
    const on = (track.previousElementSibling as HTMLInputElement | null)?.checked ? 'on' : 'off';
    const ring = getComputedStyle(track).boxShadow.match(/rgba?\([^)]*\)|color\([^)]*\)/)?.[0];
    if (ring) check(track, 'switch edge against the page', on, paintOn(ring, track), outside(track));
    else failures.push({ what: describe(track), part: 'switch edge missing', state: on, fg: '', bg: '', ratio: 0 });
    const knob = getComputedStyle(track, '::after');
    check(track, 'switch knob on its track', on, paintOn(knob.backgroundColor, track), inside(track));
  }
  for (const bar of document.body.querySelectorAll('.cap-bar, .res-bar')) {
    const fill = bar.querySelector('i');
    if (!fill || !painted(bar) || fill.getBoundingClientRect().width <= 1) continue;
    check(bar, 'capacity fill on its track', 'rest', paintOn(getComputedStyle(fill).backgroundColor, bar), inside(bar));
  }
  return failures;
}
