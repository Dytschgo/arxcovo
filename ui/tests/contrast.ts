import type { Page } from '@playwright/test';

/** One text run below the threshold, with what was measured and in which animation phase. */
export interface ContrastFailure { what: string; text: string; phase: string; fg: string; bg: string; ratio: number }

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
 * text included, in every frame).
 *
 * It measures painted text whatever its accessibility role (aria-hidden text
 * is still seen): every element's own text, generated `::before`/`::after`
 * text, form-control values and shown placeholders. The text colour's alpha,
 * the pseudo-element's own opacity and the opacity of the element and every
 * ancestor are multiplied in, and the result is composited over the painted
 * background: translucent fills (each faded by its own opacity chain) are
 * blended walking up to the pure black page.
 *
 * Every finite CSS transition or animation running at the time is sampled at
 * its start, middle and end, so a colour or fill caught mid-change is
 * measured too; they are finished afterwards. Each failure keeps its worst
 * phase.
 */
export async function contrastFailures(page: Page, minimum = 4.5): Promise<ContrastFailure[]> {
  return page.evaluate(min => {
    type Rgba = [number, number, number, number];
    const parse = (value: string): Rgba => {
      if (value === 'transparent') return [0, 0, 0, 0];
      const nums = (value.match(/-?[\d.]+(?:e-?\d+)?%?/g) ?? []).map(n => (n.endsWith('%') ? parseFloat(n) / 100 : parseFloat(n)));
      if (value.startsWith('color(srgb')) return [nums[0] * 255, nums[1] * 255, nums[2] * 255, nums[3] ?? 1];
      if (value.startsWith('rgb')) return [nums[0], nums[1], nums[2], nums[3] ?? 1];
      throw new Error(`unparsed colour ${value}`);
    };
    const over = (top: Rgba, under: Rgba): Rgba => {
      const a = top[3] + under[3] * (1 - top[3]);
      if (a === 0) return [0, 0, 0, 0];
      const mix = (i: number) => (top[i] * top[3] + under[i] * under[3] * (1 - top[3])) / a;
      return [mix(0), mix(1), mix(2), a];
    };
    const lum = ([r, g, b]: Rgba) => {
      const lin = (c: number) => { const s = c / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
      return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    };
    const ratio = (a: Rgba, b: Rgba) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
    const hex = ([r, g, b]: Rgba) => '#' + [r, g, b].map(c => Math.round(c).toString(16).padStart(2, '0')).join('');

    const opacity = (el: Element) => {
      let o = 1;
      for (let node: Element | null = el; node; node = node.parentElement) o *= parseFloat(getComputedStyle(node).opacity);
      return o;
    };
    const background = (el: Element): Rgba => {
      const layers: Rgba[] = [];
      for (let node: Element | null = el; node; node = node.parentElement) {
        const c = parse(getComputedStyle(node).backgroundColor);
        const faded: Rgba = [c[0], c[1], c[2], c[3] * opacity(node)];
        if (faded[3] > 0) layers.push(faded);
        if (faded[3] >= 1) break;
      }
      let result: Rgba = [0, 0, 0, 1];
      for (let i = layers.length - 1; i >= 0; i--) result = over(layers[i], result);
      return result;
    };
    // Painted, not merely exposed: aria-hidden text still counts. Only boxes
    // that paint nothing (clipped to 1px, hidden, not displayed) are skipped.
    const painted = (el: Element) => {
      const r = el.getBoundingClientRect();
      if (r.width <= 1 || r.height <= 1) return false;
      const s = getComputedStyle(el);
      return s.visibility !== 'hidden' && s.display !== 'none';
    };
    const describe = (el: Element) => el.tagName.toLowerCase() + (typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).join('.') : '');

    const failures = new Map<string, { what: string; text: string; phase: string; fg: string; bg: string; ratio: number }>();
    const measure = (phase: string) => {
      const check = (el: Element, what: string, text: string, colour: string, extraOpacity = 1) => {
        const bg = background(el);
        const fg = parse(colour);
        const ink = over([fg[0], fg[1], fg[2], fg[3] * extraOpacity * opacity(el)], bg);
        const r = Math.round(ratio(ink, bg) * 100) / 100;
        const key = `${what}|${text}`;
        if (r < min && (!failures.has(key) || failures.get(key)!.ratio > r)) failures.set(key, { what, text: text.slice(0, 60), phase, fg: hex(ink), bg: hex(bg), ratio: r });
      };
      for (const el of document.body.querySelectorAll('*')) {
        if (!painted(el)) continue;
        for (const pseudo of ['::before', '::after'] as const) {
          const ps = getComputedStyle(el, pseudo);
          const content = ps.content.match(/^"(.+)"$/)?.[1];
          if (content && content.trim()) check(el, describe(el) + pseudo, content, ps.color, parseFloat(ps.opacity));
        }
        if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
          if (el.type === 'checkbox' || el.type === 'radio' || el.type === 'hidden') continue;
          if (el.value) check(el, describe(el), el.type === 'password' ? '•••' : el.value, getComputedStyle(el).color);
          else if (el.placeholder) {
            const ph = getComputedStyle(el, '::placeholder');
            check(el, `${describe(el)}::placeholder`, el.placeholder, ph.color, parseFloat(ph.opacity));
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
    return [...failures.values()];
  }, minimum);
}
