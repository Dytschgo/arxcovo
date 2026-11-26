import type { Page } from '@playwright/test';
import { compositing, type Compositing, type Layer, type Rgba } from './compositing';

/** One user-interface part below 3:1 against what it adjoins (WCAG 2.2 SC 1.4.11). */
export interface NonTextFailure { what: string; part: string; state: string; fg: string; bg: string; ratio: number; detail?: string }

/**
 * Rendered non-text contrast audit (WCAG 2.2 SC 1.4.11, 3:1): the visual
 * information that identifies a component or its state, measured on the
 * painted page with the same group compositing as the text audit.
 *
 * - Enabled text fields and selects: the border (against both the surface
 *   outside it and its own fill) or the fill (against the outside) is the
 *   boundary that shows where to type. Disabled controls are exempt.
 * - Checkboxes and radios: the boundary at rest, the fill when checked and the
 *   check mark on that fill. A natively drawn box (`appearance` not `none`)
 *   fails: its edge is not measurable and differs per browser and theme.
 * - The current or selected item of a set (`aria-current`, `aria-selected`):
 *   its fill or border against the set it sits in.
 * - Icons (`svg.i`, `.os-glyph`), kind markers and meter fills against what
 *   they are painted on.
 * - Text-link underlines at rest: where a link's colour matches the text
 *   around it, its underline is the only cue that it is a link, so the
 *   decoration colour must reach 3:1 against the surface under it.
 * - Focus indicators: every Tab stop is reached with the real keyboard; the
 *   outline (on the element or the ancestor that draws it) or border that
 *   appears on focus must reach 3:1 against the colour beside it.
 *
 * The page's focus is restored afterwards.
 */
export async function nonTextFailures(page: Page, minimum = 3): Promise<NonTextFailure[]> {
  const parts = await page.evaluate(`(${partsInPage.toString()})((${compositing.toString()})(), ${JSON.stringify(minimum)})`) as NonTextFailure[];
  return [...parts, ...await focusIndicatorFailures(page, minimum)];
}

/** The focus half of {@link nonTextFailures}. */
export async function focusIndicatorFailures(page: Page, minimum = 3): Promise<NonTextFailure[]> {
  await page.evaluate(`(${focusRestInPage.toString()})()`);
  const failures: NonTextFailure[] = [];
  for (let step = 0; step < 150; step++) {
    await page.keyboard.press('Tab');
    const result = await page.evaluate(`(${focusStepInPage.toString()})((${compositing.toString()})(), ${JSON.stringify(minimum)})`) as { done: boolean; failure: NonTextFailure | null };
    if (result.failure) failures.push(result.failure);
    if (result.done) break;
  }
  await page.evaluate(`(${focusRestoreInPage.toString()})()`);
  return failures;
}

// The functions below run in the page: only their arguments and browser globals.

function partsInPage(C: Compositing, min: number): NonTextFailure[] {
  const describe = (el: Element) => el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).join('.') : '')
    + (el.getAttribute('aria-label') ? `[aria-label="${el.getAttribute('aria-label')}"]` : '');
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
  const border = (el: Element) => {
    const s = getComputedStyle(el);
    return s.borderTopStyle !== 'none' && s.borderTopStyle !== 'hidden' && parseFloat(s.borderTopWidth) > 0 ? s.borderTopColor : null;
  };
  const painted = (el: Element) => {
    const r = el.getBoundingClientRect();
    if (r.width <= 1 || r.height <= 1) return false;
    const s = getComputedStyle(el);
    return s.visibility !== 'hidden' && s.display !== 'none';
  };

  const failures = new Map<string, NonTextFailure & { raw: number }>();
  const shown = (raw: number) => Math.floor(raw * 100) / 100;
  const report = (el: Element, part: string, state: string, fg: Rgba, bg: Rgba, raw: number, detail?: string) => {
    if (raw >= min) return;
    const key = `${describe(el)}|${part}`;
    if (!failures.has(key) || failures.get(key)!.raw > raw) failures.set(key, { what: describe(el), part, state, fg: C.hex(fg), bg: C.hex(bg), ratio: shown(raw), raw, detail });
  };
  const state = (el: Element) => (el === document.activeElement ? 'focus' : el.getAttribute('aria-invalid') === 'true' ? 'invalid' : 'rest');

  /** A boundary is the border (against outside and inside) or the fill (against outside). */
  const boundary = (el: Element, part: string, st: string) => {
    const out = outside(el);
    const fill = inside(el);
    const fillRatio = C.ratio(fill, out);
    const b = border(el);
    let best = { fg: fill, bg: out, raw: fillRatio };
    let detail = `fill ${C.hex(fill)} ${shown(fillRatio)} vs outside ${C.hex(out)}`;
    if (b) {
      const edge = paintOn(b, el);
      const [ro, ri] = [C.ratio(edge, out), C.ratio(edge, fill)];
      const raw = Math.min(ro, ri);
      if (raw > best.raw) best = { fg: edge, bg: ro <= ri ? out : fill, raw };
      detail = `border ${C.hex(edge)} ${shown(ro)} vs outside ${C.hex(out)}, ${shown(ri)} vs own fill ${C.hex(fill)}; ${detail}`;
    }
    report(el, part, st, best.fg, best.bg, best.raw, detail);
  };

  const entry = 'input:not([type=checkbox]):not([type=radio]):not([type=hidden]):not([type=button]):not([type=submit]):not([type=reset]):not([type=image]):not([type=range]):not([type=color]):not([type=file]), select, textarea';
  for (const el of document.body.querySelectorAll<HTMLInputElement>(entry)) {
    if (!painted(el) || el.disabled) continue;
    boundary(el, 'field boundary', state(el));
  }

  for (const el of document.body.querySelectorAll<HTMLInputElement>('input[type=checkbox], input[type=radio]')) {
    if (!painted(el) || el.disabled) continue;
    if (getComputedStyle(el).appearance !== 'none') {
      report(el, 'natively drawn box (style it with appearance: none so its edge is measurable)', state(el), clear, clear, 0);
      continue;
    }
    if (el.checked) {
      report(el, 'checked fill', 'checked', inside(el), outside(el), C.ratio(inside(el), outside(el)));
      const mark = getComputedStyle(el, '::before');
      const markColour = mark.borderBottomStyle !== 'none' && parseFloat(mark.borderBottomWidth) > 0 ? mark.borderBottomColor : mark.backgroundColor;
      if (mark.content === 'none' || mark.content === 'normal') report(el, 'check mark missing', 'checked', clear, clear, 0);
      else {
        const ink = paintOn(markColour, el);
        report(el, 'check mark', 'checked', ink, inside(el), C.ratio(ink, inside(el)));
      }
    } else boundary(el, 'box boundary', state(el));
  }

  for (const el of document.body.querySelectorAll('[aria-current]:not([aria-current="false"]), [aria-selected="true"]')) {
    if (!painted(el)) continue;
    boundary(el, 'selected-state fill or border', 'selected');
  }

  for (const el of document.body.querySelectorAll('svg.i, svg.os-glyph')) {
    if (!painted(el)) continue;
    const ink = paintOn(getComputedStyle(el).color, el);
    const bg = inside(el);
    report(el, 'icon', 'rest', ink, bg, C.ratio(ink, bg));
  }

  for (const el of document.body.querySelectorAll('.kind')) {
    if (!painted(el)) continue;
    // A ring (border) or a solid diamond (fill), either way against the surface around it.
    const b = border(el);
    const ink = b ? paintOn(b, el) : inside(el);
    report(el, 'kind marker', 'rest', ink, outside(el), C.ratio(ink, outside(el)));
  }

  for (const track of document.body.querySelectorAll('[role=meter]')) {
    const fill = track.firstElementChild;
    if (!fill || !painted(track) || fill.getBoundingClientRect().width <= 1) continue;
    const ink = C.composite(C.parse(getComputedStyle(fill).backgroundColor), chain(track)).ink;
    report(track, 'meter fill on its track', 'rest', ink, inside(track), C.ratio(ink, inside(track)));
  }

  for (const link of document.body.querySelectorAll('a[href]')) {
    if (!painted(link)) continue;
    const s = getComputedStyle(link);
    if (!s.textDecorationLine.includes('underline')) continue;
    const ink = paintOn(s.textDecorationColor, link);
    const bg = inside(link);
    const text = (link.textContent ?? '').trim().slice(0, 40);
    report(link, `link underline "${text}"`, 'rest', ink, bg, C.ratio(ink, bg));
  }

  return [...failures.values()].map(f => ({ what: f.what, part: f.part, state: f.state, fg: f.fg, bg: f.bg, ratio: f.ratio, detail: f.detail }));
}

function focusRestInPage(): void {
  type Rest = { outline: string; border: string };
  const w = window as unknown as { __arxFocus?: { rest: WeakMap<Element, Rest>; seen: WeakSet<Element>; before: Element | null } };
  const rest = new WeakMap<Element, Rest>();
  const before = document.activeElement;
  (before as HTMLElement | null)?.blur?.();
  for (const el of document.querySelectorAll('*')) {
    const s = getComputedStyle(el);
    rest.set(el, { outline: `${s.outlineStyle}|${s.outlineWidth}|${s.outlineColor}|${s.outlineOffset}`, border: s.borderTopColor });
  }
  w.__arxFocus = { rest, seen: new WeakSet(), before };
}

function focusStepInPage(C: Compositing, min: number): { done: boolean; failure: NonTextFailure | null } {
  const w = window as unknown as { __arxFocus: { rest: WeakMap<Element, { outline: string; border: string }>; seen: WeakSet<Element> } };
  const el = document.activeElement;
  if (!el || el === document.body || el === document.documentElement) return { done: false, failure: null };
  if (w.__arxFocus.seen.has(el)) return { done: true, failure: null };
  w.__arxFocus.seen.add(el);

  const describe = (e: Element) => e.tagName.toLowerCase() + (typeof e.className === 'string' && e.className.trim() ? '.' + e.className.trim().split(/\s+/).join('.') : '')
    + ((e.textContent ?? '').trim() ? ` "${(e.textContent ?? '').trim().slice(0, 30)}"` : e.getAttribute('aria-label') ? `[aria-label="${e.getAttribute('aria-label')}"]` : '');
  const chain = (e: Element | null): Layer[] => {
    const out: Layer[] = [];
    for (let node = e; node; node = node.parentElement) {
      const s = getComputedStyle(node);
      out.push({ background: C.parse(s.backgroundColor), opacity: parseFloat(s.opacity) });
    }
    return out;
  };
  const clear: Rgba = [0, 0, 0, 0];
  const what = describe(el);

  // Candidates that appeared with focus: an outline on the element or on the
  // ancestor that draws it (a card around its stretched link), or a recoloured border.
  let best: { fg: Rgba; bg: Rgba; raw: number; part: string } | null = null;
  for (let node: Element | null = el, depth = 0; node && depth < 4; node = node.parentElement, depth++) {
    const s = getComputedStyle(node);
    const rest = w.__arxFocus.rest.get(node);
    const outline = `${s.outlineStyle}|${s.outlineWidth}|${s.outlineColor}|${s.outlineOffset}`;
    if (s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0 && outline !== rest?.outline) {
      const offset = parseFloat(s.outlineOffset);
      // The outline sits beside the box (offset >= 0) or inside it (negative offset).
      const under = offset >= 0 ? [{ background: clear, opacity: parseFloat(s.opacity) }, ...chain(node.parentElement)] : chain(node);
      const { ink, background } = C.composite(C.parse(s.outlineColor), under);
      const raw = C.ratio(ink, background);
      if (!best || raw > best.raw) best = { fg: ink, bg: background, raw, part: 'focus outline' };
    }
    if (node === el && rest && s.borderTopColor !== rest.border && s.borderTopStyle !== 'none' && parseFloat(s.borderTopWidth) > 0) {
      const ink = C.composite(C.parse(s.borderTopColor), chain(node)).ink;
      const out = C.composite(clear, chain(node.parentElement)).background;
      const fill = C.composite(clear, chain(node)).background;
      const raw = Math.min(C.ratio(ink, out), C.ratio(ink, fill));
      if (!best || raw > best.raw) best = { fg: ink, bg: C.ratio(ink, out) <= C.ratio(ink, fill) ? out : fill, raw, part: 'focus border' };
    }
  }
  if (!best) return { done: false, failure: { what, part: 'no focus indicator appears', state: 'focus', fg: '', bg: '', ratio: 0 } };
  if (best.raw >= min) return { done: false, failure: null };
  return { done: false, failure: { what, part: best.part, state: 'focus', fg: C.hex(best.fg), bg: C.hex(best.bg), ratio: Math.floor(best.raw * 100) / 100 } };
}

function focusRestoreInPage(): void {
  const w = window as unknown as { __arxFocus?: { before: Element | null } };
  const before = w.__arxFocus?.before as HTMLElement | null | undefined;
  (document.activeElement as HTMLElement | null)?.blur?.();
  if (before && before !== document.body && before.isConnected) before.focus({ preventScroll: true });
  delete w.__arxFocus;
}
