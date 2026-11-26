import type { Page } from '@playwright/test';

/** One text run below the threshold, with what was measured. */
export interface ContrastFailure { what: string; text: string; fg: string; bg: string; ratio: number }

/**
 * Rendered WCAG contrast audit (Night Orbit rule: all text >= 4.5:1, large
 * text included). For every visible element with its own text, every
 * form-control value and every shown placeholder, it takes the computed
 * colour, multiplies in the opacity of the element and its ancestors, and
 * composites it over the first opaque background found walking up, with
 * translucent backgrounds blended on the way, once finite transitions and
 * animations have finished. So it measures what Chromium actually paints,
 * including UA defaults such as the placeholder colour, not only the declared
 * tokens.
 */
export async function contrastFailures(page: Page, minimum = 4.5): Promise<ContrastFailure[]> {
  return page.evaluate(async min => {
    // Measure settled paint: wait for every finite CSS transition and
    // animation (a just-disabled control fades to its disabled style).
    await Promise.all(document.getAnimations()
      .filter(a => a.effect?.getComputedTiming().endTime !== Infinity)
      .map(a => a.finished.catch(() => undefined)));
    type Rgba = [number, number, number, number];
    const parse = (value: string): Rgba => {
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
    // A background is faded by its element's opacity and every ancestor's
    // (opacity groups composite whole subtrees); the page under all is black.
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
    const visible = (el: Element) => {
      const r = el.getBoundingClientRect();
      if (r.width <= 1 || r.height <= 1) return false; // includes .visually-hidden
      const s = getComputedStyle(el);
      return s.visibility !== 'hidden' && s.display !== 'none' && el.closest('[aria-hidden="true"]') === null;
    };

    const failures: { what: string; text: string; fg: string; bg: string; ratio: number }[] = [];
    const check = (el: Element, what: string, text: string, colour: string) => {
      const bg = background(el);
      const fg = parse(colour);
      const painted = over([fg[0], fg[1], fg[2], fg[3] * opacity(el)], bg);
      const r = ratio(painted, bg);
      if (r < min) failures.push({ what, text: text.slice(0, 60), fg: hex(painted), bg: hex(bg), ratio: Math.round(r * 100) / 100 });
    };
    const describe = (el: Element) => el.tagName.toLowerCase() + (el.className && typeof el.className === 'string' ? '.' + el.className.trim().split(/\s+/).join('.') : '');

    for (const el of document.body.querySelectorAll('*')) {
      if (!visible(el)) continue;
      if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
        if (el.type === 'checkbox' || el.type === 'radio' || el.type === 'hidden') continue;
        if (el.value) check(el, describe(el), el.type === 'password' ? '•••' : el.value, getComputedStyle(el).color);
        else if (el.placeholder) check(el, `${describe(el)}::placeholder`, el.placeholder, getComputedStyle(el, '::placeholder').color);
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
    return failures;
  }, minimum);
}
