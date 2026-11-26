import type { Page } from '@playwright/test';
import { compositing, type Compositing, type Layer } from './compositing';

/** One text run below the threshold, with what was measured and in which animation phase. */
export interface ContrastFailure { what: string; text: string; phase: string; fg: string; bg: string; ratio: number }

/**
 * A text run painted inside an opacity group (`opacity` < 1 on the box that
 * draws it, on its pseudo-element, or on any ancestor). Night Orbit never dims
 * text or anything around text with opacity; only decorative, aria-hidden
 * graphics without text (the logo light) may fade.
 */
export interface OpacityGroupOverText { what: string; text: string; phase: string; group: string; opacity: number }

/** Both rendered text guards from one pass over the page. */
export interface TextAudit { contrast: ContrastFailure[]; opacityGroups: OpacityGroupOverText[] }

/**
 * Freezes the page's animation timeline (CSS transitions and animations stay
 * at their first frame until the audit samples them). Call once per page,
 * before the actions whose transitions should be measured.
 */
export async function freezeMotion(page: Page): Promise<void> {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Animation.enable');
  await cdp.send('Animation.setPlaybackRate', { playbackRate: 0 });
}

/**
 * Rendered WCAG contrast audit (Night Orbit rule: all text >= 4.5:1, large
 * text included, in every frame) plus the opacity guard, in one pass.
 *
 * It measures painted text whatever its accessibility role (aria-hidden text
 * is still seen): every element's own text, generated `::before`/`::after`
 * text, form-control values and shown placeholders. For each run it collects
 * the boxes from the one that draws it (the pseudo-element first, if any) up
 * to the root, each with its fill and its `opacity`, and composites them the
 * way Chromium does (see `composite` in ./compositing.ts): every opacity
 * group fades its whole painted subtree once, level by level. The unrounded
 * ratio is compared with the minimum; the reported ratio is rounded down for
 * display, so a failure never reads as passing.
 *
 * `opacityGroups` lists every text run that has any opacity < 1 on that
 * chain, whatever its ratio: the structural guard that keeps the product off
 * this path entirely.
 *
 * Every finite CSS transition or animation running at the time is sampled at
 * its start, middle and end, so a colour, fill or opacity caught mid-change
 * is measured too; they are finished afterwards. Each finding keeps its worst
 * phase.
 */
export async function auditText(page: Page, minimum = 4.5): Promise<TextAudit> {
  // Injected as source so the page runs exactly the maths Vitest checks.
  return page.evaluate(`(${inPage.toString()})((${compositing.toString()})(), ${JSON.stringify(minimum)})`) as Promise<TextAudit>;
}

/** The contrast half of {@link auditText}. */
export async function contrastFailures(page: Page, minimum = 4.5): Promise<ContrastFailure[]> {
  return (await auditText(page, minimum)).contrast;
}

// Runs in the page: it may use only its arguments and browser globals.
function inPage(C: Compositing, min: number): TextAudit {
  type Box = Layer & { name: string };
  const describe = (el: Element) => el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).join('.') : '');
  const boxes = (el: Element, pseudo?: { name: string; style: CSSStyleDeclaration }): Box[] => {
    const out: Box[] = [];
    if (pseudo) out.push({ name: describe(el) + pseudo.name, background: C.parse(pseudo.style.backgroundColor), opacity: parseFloat(pseudo.style.opacity) });
    for (let node: Element | null = el; node; node = node.parentElement) {
      const s = getComputedStyle(node);
      out.push({ name: describe(node), background: C.parse(s.backgroundColor), opacity: parseFloat(s.opacity) });
    }
    return out;
  };
  // Painted, not merely exposed: aria-hidden text still counts. Only boxes
  // that paint nothing (clipped to 1px, hidden, not displayed) are skipped.
  const painted = (el: Element) => {
    const r = el.getBoundingClientRect();
    if (r.width <= 1 || r.height <= 1) return false;
    const s = getComputedStyle(el);
    return s.visibility !== 'hidden' && s.display !== 'none';
  };

  const contrast = new Map<string, ContrastFailure & { raw: number }>();
  const groups = new Map<string, OpacityGroupOverText>();
  const measure = (phase: string) => {
    const check = (el: Element, what: string, text: string, colour: string, pseudo?: { name: string; style: CSSStyleDeclaration }) => {
      const chain = boxes(el, pseudo);
      const { ink, background } = C.composite(C.parse(colour), chain);
      const raw = C.ratio(ink, background);
      const key = `${what}|${text}`;
      if (raw < min && (!contrast.has(key) || contrast.get(key)!.raw > raw)) {
        contrast.set(key, { what, text: text.slice(0, 60), phase, fg: C.hex(ink), bg: C.hex(background), ratio: Math.floor(raw * 100) / 100, raw });
      }
      for (const box of chain) {
        if (!(box.opacity < 1)) continue;
        const gkey = `${key}|${box.name}`;
        if (!groups.has(gkey) || groups.get(gkey)!.opacity > box.opacity) groups.set(gkey, { what, text: text.slice(0, 60), phase, group: box.name, opacity: box.opacity });
      }
    };
    for (const el of document.body.querySelectorAll('*')) {
      if (!painted(el)) continue;
      for (const name of ['::before', '::after'] as const) {
        const style = getComputedStyle(el, name);
        const content = style.content.match(/^"(.+)"$/)?.[1];
        if (content && content.trim()) check(el, describe(el) + name, content, style.color, { name, style });
      }
      if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
        if (el.type === 'checkbox' || el.type === 'radio' || el.type === 'hidden') continue;
        if (el.value) check(el, describe(el), el.type === 'password' ? '•••' : el.value, getComputedStyle(el).color);
        else if (el.placeholder) {
          const style = getComputedStyle(el, '::placeholder');
          check(el, `${describe(el)}::placeholder`, el.placeholder, style.color, { name: '::placeholder', style });
        }
        continue;
      }
      if (el instanceof HTMLSelectElement) {
        check(el, describe(el), el.selectedOptions[0]?.text ?? '', getComputedStyle(el).color);
        continue;
      }
      if (el instanceof HTMLOptionElement) continue; // drawn by the native popup
      const own = [...el.childNodes].filter(n => n.nodeType === Node.TEXT_NODE).map(n => n.textContent ?? '').join('').trim();
      if (own) check(el, describe(el), own, getComputedStyle(el).color);
    }
  };

  const finite = document.getAnimations().filter(a => {
    const end = a.effect?.getComputedTiming().endTime;
    return end !== undefined && Number.isFinite(Number(end));
  });
  if (finite.length === 0) measure('settled');
  else {
    for (const [phase, at] of [['start', 0], ['middle', 0.5], ['end', 1]] as const) {
      for (const a of finite) {
        a.pause();
        a.currentTime = Number(a.effect!.getComputedTiming().endTime) * at;
      }
      measure(phase);
    }
    for (const a of finite) a.finish();
  }
  return {
    contrast: [...contrast.values()].map(f => ({ what: f.what, text: f.text, phase: f.phase, fg: f.fg, bg: f.bg, ratio: f.ratio })),
    opacityGroups: [...groups.values()],
  };
}
