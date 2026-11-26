// Guards for the Night Orbit stylesheet: text contrast, self-hosted fonts,
// dark only, and the reduced-motion finished state.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

// Paths are joined by hand: Vite rewrites `new URL(..., import.meta.url)` into asset URLs.
const here = dirname(fileURLToPath(import.meta.url));
const read = (rel: string) => readFileSync(join(here, rel), 'utf8');
const css = read('./styles.css');
const html = read('../index.html');

function token(name: string): string {
  const match = css.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{3,6})\\s*;`));
  if (!match) throw new Error(`token --${name} missing`);
  return match[1];
}

function luminance(hex: string): number {
  const h = hex.length === 4 ? hex.slice(1).split('').map(c => c + c).join('') : hex.slice(1);
  const [r, g, b] = [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16) / 255)
    .map(c => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

describe('Night Orbit tokens', () => {
  test.each(['ink', 'ink2', 'ink3', 'ink4'])('--%s clears 4.5:1 on every surface text sits on', ink => {
    for (const surface of ['bg', 'tile', 'tile2', 'raise']) {
      expect(contrast(token(ink), token(surface)), `--${ink} on --${surface}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  test('the decoration grey is not text-safe, so it stays decoration only', () => {
    expect(contrast(token('deco'), token('bg'))).toBeLessThan(4.5);
    expect(css).not.toMatch(/(?:^|[;{\s])color:\s*var\(--deco\)/m);
  });
});

/** Top-level and @media rules as selector -> declarations (comments stripped). */
function rules(): { selector: string; decls: Map<string, string> }[] {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, '');
  return [...text.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(m => ({
    selector: m[1].replace(/@media[^{]*$/, '').trim(),
    decls: new Map(m[2].split(';').map(d => d.split(':')).filter(p => p.length >= 2).map(([k, ...v]) => [k.trim(), v.join(':').trim()])),
  }));
}
const tokenOf = (value: string | undefined) => value?.match(/^var\(--([\w-]+)\)$/)?.[1];
const rule = (selector: string) => {
  const found = rules().find(r => r.selector.split(',').map(x => x.trim()).includes(selector));
  if (!found) throw new Error(`no rule for ${selector}`);
  return found.decls;
};

describe('text on its real backgrounds', () => {
  // Every pair below is a text colour drawn on a fill other than the page:
  // placeholders and disabled controls sit on input fills (--tile2) and on
  // tiles; the current nav item and the primary button are black on white.
  test.each([
    ['::placeholder', ['tile2', 'tile', 'bg']],
    ['.button:disabled', ['bg', 'tile', 'tile2']],
    ['.button.button-disabled', ['bg', 'tile', 'tile2']],
    ['.terminal-input input:disabled', ['tile2']],
    ['.field select:disabled', ['tile2']],
    ['.button-primary', ['ink']],
    ['.nav a[aria-current="page"]', ['ink']],
  ] as const)('%s clears 4.5:1 on its fills', (selector, fills) => {
    const decls = rule(selector);
    const fg = tokenOf(decls.get('color'));
    expect(fg, `${selector} sets a token colour`).toBeDefined();
    for (const fill of fills) expect(contrast(token(fg!), token(fill)), `${selector} on --${fill}`).toBeGreaterThanOrEqual(4.5);
  });

  test('placeholders are fully opaque, not the UA default faded grey', () => {
    expect(rule('::placeholder').get('opacity')).toBe('1');
  });

  test('every text colour is a text-safe ink token (or black on a white fill)', () => {
    const allowed = new Set(['var(--ink)', 'var(--ink2)', 'var(--ink3)', 'var(--ink4)', 'var(--bg)', 'inherit', 'currentColor']);
    const bad = rules().flatMap(r => (r.decls.has('color') && !allowed.has(r.decls.get('color')!) && !/os-glyph/.test(r.selector) ? [`${r.selector}: ${r.decls.get('color')}`] : []));
    expect(bad).toEqual([]);
  });

  test('a rule that sets both text and fill tokens pairs them at 4.5:1 or better', () => {
    for (const r of rules()) {
      const fg = tokenOf(r.decls.get('color'));
      const bg = tokenOf(r.decls.get('background') ?? r.decls.get('background-color'));
      if (fg && bg) expect(contrast(token(fg), token(bg)), r.selector).toBeGreaterThanOrEqual(4.5);
    }
  });

  test('nothing that holds text is dimmed with opacity (only entrance animations and the logo light start at 0)', () => {
    const dimmed = rules().filter(r => {
      const o = r.decls.get('opacity');
      return o !== undefined && parseFloat(o) < 1 && !/^(\.rise|\.logo \.lg-c|from|to|\d+%)/.test(r.selector);
    }).map(r => r.selector);
    expect(dimmed).toEqual([]);
  });
});

describe('fonts', () => {
  test('Geist and Geist Mono are self-hosted with their licence, and nothing loads from another origin', () => {
    const sources = [...css.matchAll(/url\(\s*["']?([^"')]+)["']?\s*\)/g)].map(m => m[1]);
    expect(sources).toEqual(['/fonts/Geist-Variable.woff2', '/fonts/GeistMono-Variable.woff2']);
    for (const file of ['Geist-Variable.woff2', 'GeistMono-Variable.woff2', 'OFL-LICENSE.txt']) {
      expect(existsSync(join(here, '../public/fonts', file)), file).toBe(true);
    }
    expect(read('../public/fonts/OFL-LICENSE.txt')).toContain('SIL Open Font License, Version 1.1');
    expect(css).not.toMatch(/@import|https?:\/\//);
    expect(html).not.toMatch(/https?:\/\/(?!www\.w3\.org)/);
    expect(css).toMatch(/--font:\s*"Geist"/);
    expect(css).toMatch(/--mono:\s*"Geist Mono"/);
  });
});

describe('dark only', () => {
  test('there is no light scheme, theme attribute or scheme query', () => {
    expect(css).toMatch(/:root\s*{[^}]*color-scheme:\s*dark;/);
    expect(css).not.toMatch(/light-dark\(|prefers-color-scheme|data-theme|color-scheme:\s*light/);
    expect(html).toContain('<meta name="color-scheme" content="dark">');
  });
});

describe('motion', () => {
  test('reduced motion shows the finished state', () => {
    const reduce = css.slice(css.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(reduce).toMatch(/animation-duration:\s*\.01ms\s*!important/);
    expect(reduce).toMatch(/animation-delay:\s*0ms\s*!important/);
    expect(reduce).toMatch(/transition-duration:\s*\.01ms\s*!important/);
    expect(reduce).toMatch(/\.rise\s*{\s*opacity:\s*1;\s*transform:\s*none;\s*}/);
  });
});
