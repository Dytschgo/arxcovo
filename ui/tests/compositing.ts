/** Straight (non-premultiplied) sRGB colour: r, g, b in 0–255 and alpha in 0–1. */
export type Rgba = [number, number, number, number];

/** One box on the way from a text run up to the page: its own fill and its own `opacity`. */
export interface Layer { background: Rgba; opacity: number }

/** The two final pixels of a text run: inside a glyph and right next to it. */
export interface Composite { ink: Rgba; background: Rgba }

/**
 * Colour maths for the rendered contrast audit (./contrast.ts).
 *
 * The factory is self-contained on purpose: the audit injects its source into
 * the page, and Vitest calls it directly (src/contrast-compositing.test.ts),
 * so the browser runs exactly the maths the unit tests check.
 */
export function compositing() {
  /** Source-over: `top` painted on `under`. */
  const over = (top: Rgba, under: Rgba): Rgba => {
    const a = top[3] + under[3] * (1 - top[3]);
    if (a === 0) return [0, 0, 0, 0];
    const mix = (i: number) => (top[i] * top[3] + under[i] * under[3] * (1 - top[3])) / a;
    return [mix(0), mix(1), mix(2), a];
  };

  /** A group's opacity scales the alpha of everything the group painted, once. */
  const fade = (c: Rgba, opacity: number): Rgba => [c[0], c[1], c[2], c[3] * opacity];

  /**
   * Final glyph and background pixels of a text run, the way Chromium paints
   * them. `layers` runs from the box that draws the text (a pseudo-element
   * first, when the text is generated or a placeholder) up to the root.
   *
   * Each box paints its fill, then its children on top; its `opacity` then
   * fades that whole painted subtree as one group before it lands on the
   * parent's fill. So the glyph pixel is text over the box's fill, and the
   * pixel beside it is the fill alone, both carried up level by level and
   * finally painted on the page canvas (opaque black in Night Orbit).
   *
   * Fading text and fill separately (alpha multiplied into each) is wrong as
   * soon as an ancestor below an opacity group has a fill: the group's fill
   * would be counted under the glyph twice, which reports a higher contrast
   * than the screen shows.
   */
  const composite = (text: Rgba, layers: readonly Layer[], page: Rgba = [0, 0, 0, 1]): Composite => {
    let ink = text;
    let background: Rgba = [0, 0, 0, 0];
    for (const layer of layers) {
      ink = fade(over(ink, layer.background), layer.opacity);
      background = fade(over(background, layer.background), layer.opacity);
    }
    return { ink: over(ink, page), background: over(background, page) };
  };

  const luminance = ([r, g, b]: Rgba) => {
    const lin = (c: number) => { const s = c / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  };

  /** WCAG contrast ratio, unrounded (compare this; round only for display). */
  const ratio = (a: Rgba, b: Rgba) => {
    const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
    return (x + 0.05) / (y + 0.05);
  };

  /** Parses a computed CSS colour (`rgb()`, `rgba()`, `color(srgb …)`, `transparent`). */
  const parse = (value: string): Rgba => {
    if (value === 'transparent') return [0, 0, 0, 0];
    const nums = (value.match(/-?[\d.]+(?:e-?\d+)?%?/g) ?? []).map(n => (n.endsWith('%') ? parseFloat(n) / 100 : parseFloat(n)));
    if (value.startsWith('color(srgb')) return [nums[0] * 255, nums[1] * 255, nums[2] * 255, nums[3] ?? 1];
    if (value.startsWith('rgb')) return [nums[0], nums[1], nums[2], nums[3] ?? 1];
    throw new Error(`unparsed colour ${value}`);
  };

  const hex = ([r, g, b]: Rgba) => '#' + [r, g, b].map(c => Math.round(c).toString(16).padStart(2, '0')).join('');

  return { over, fade, composite, luminance, ratio, parse, hex };
}

export type Compositing = ReturnType<typeof compositing>;
