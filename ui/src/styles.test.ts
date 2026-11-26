// Guards for the Night Orbit stylesheet: text and non-text contrast,
// self-hosted fonts, dark only, and the reduced-motion finished state.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
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

  // Opacity fades a box and everything inside it as one group, text included,
  // so nothing that holds text, and no ancestor of text, may be below 1. The
  // single exception is the logo light (`.logo .lg-c`): an SVG stroke in an
  // aria-hidden graphic with no text (asserted in app/ui.test.tsx). Keyframes
  // are guarded under "motion"; the rendered audit (ui/tests/contrast.ts)
  // also fails any painted text inside an opacity group, however it got there.
  test('nothing that holds text, or contains it, is dimmed with opacity (only the logo light fades)', () => {
    const dimmed = rules().filter(r => {
      const o = r.decls.get('opacity');
      return o !== undefined && parseFloat(o) < 1 && !/^(\.logo \.lg-c|from|to|\d+%)/.test(r.selector);
    }).map(r => r.selector);
    expect(dimmed).toEqual([]);
  });

  test('components never set opacity from script or inline styles', () => {
    // A `style={{ opacity }}` or a CSSOM write would bypass the stylesheet rule above.
    const sources = (readdirSync(here, { recursive: true }) as string[])
      .filter(f => /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f) && !/test-setup\.ts$/.test(f));
    expect(sources.length).toBeGreaterThan(10);
    const offenders = sources.flatMap(f => read(f).split('\n')
      .flatMap((line, i) => (/opacity/i.test(line) ? [`${f.replace(/\\/g, '/')}:${i + 1}: ${line.trim()}`] : [])));
    expect(offenders).toEqual([]);
  });
});

// WCAG 2.2 SC 1.4.11: what identifies a control or its state reaches 3:1.
// The rendered audit (ui/tests/nontext.ts) measures the painted page; these
// pin the stylesheet so a hover, focus or invalid rule cannot quietly undo it.
describe('non-text contrast (3:1)', () => {
  const surfaces = ['bg', 'tile', 'tile2', 'raise'];
  const edgeOf = (decls: Map<string, string>) => decls.get('border-color') ?? decls.get('border')?.match(/var\(--[\w-]+\)|#[0-9a-fA-F]{3,6}\b/)?.[0];

  test('--edge, the control boundary, reaches 3:1 on every surface a control sits on', () => {
    for (const surface of surfaces) expect(contrast(token('edge'), token(surface)), `--edge on --${surface}`).toBeGreaterThanOrEqual(3);
    // ...and the hairlines it replaces for controls do not, which is why they stay decoration.
    expect(contrast(token('line2'), token('tile'))).toBeLessThan(3);
  });

  test('enabled fields and checkboxes keep a 3:1 edge in every state; disabled ones keep a dashed edge', () => {
    const controls = rules().filter(r => r.selector.split(',').some(sel => /(^|[\s>+~])(input|select|textarea)\b/.test(sel.trim()) && !/::/.test(sel)));
    const edged = controls.filter(r => edgeOf(r.decls));
    expect(edged.map(r => r.selector)).toEqual(expect.arrayContaining([
      '.field input, .field select, .terminal-input input',
      '.field input:focus-visible, .field select:focus-visible, .terminal-input input:focus-visible',
      '.check input',
      '.check input:checked',
    ]));
    for (const r of edged) {
      if (r.selector.split(',').every(sel => sel.includes(':disabled'))) {
        expect(r.decls.get('border-style'), r.selector).toBe('dashed');
        continue;
      }
      expect(['var(--edge)', 'var(--ink)'], `${r.selector} edge ${edgeOf(r.decls)}`).toContain(edgeOf(r.decls));
    }
  });

  test('checkboxes are drawn by the stylesheet (measurable), and native again under forced colours', () => {
    const box = rule('.check input');
    expect(box.get('appearance')).toBe('none');
    expect(box.get('border')).toMatch(/var\(--edge\)/);
    expect(rule('.check input:checked').get('background')).toBe('var(--ink)');
    expect(rule('.check input:checked::before').get('border')).toMatch(/var\(--bg\)/);
    const forced = css.slice(css.indexOf('@media (forced-colors: active)'));
    expect(forced).toMatch(/\.check input\s*{\s*appearance:\s*auto;\s*}/);
  });

  test('every focus ring is --ink, which reaches 3:1 on every surface', () => {
    const rings = rules().filter(r => /:focus-visible/.test(r.selector) && /solid|dashed|dotted/.test(r.decls.get('outline') ?? ''));
    expect(rings.length).toBeGreaterThan(0);
    for (const r of rings) expect(r.decls.get('outline'), r.selector).toMatch(/var\(--ink\)/);
    for (const surface of surfaces) expect(contrast(token('ink'), token(surface))).toBeGreaterThanOrEqual(3);
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
  test('reduced motion shows the finished state at once', () => {
    const reduce = css.slice(css.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(reduce).toMatch(/animation-duration:\s*0s\s*!important/);
    expect(reduce).toMatch(/animation-delay:\s*0s\s*!important/);
    expect(reduce).toMatch(/transition-duration:\s*0s\s*!important/);
    expect(reduce).toMatch(/transition-delay:\s*0s\s*!important/);
    expect(reduce).toMatch(/\.rise\s*{\s*transform:\s*none;\s*}/);
  });

  // Text must keep 4.5:1 in every frame, so motion may move things or recolour
  // borders, never change text colour, a fill behind text, or opacity.
  test('transitions only move things or recolour borders', () => {
    const allowed = new Set(['transform', 'border-color']);
    const used = rules().flatMap(r => ['transition', 'transition-property'].filter(k => r.decls.has(k))
      .flatMap(k => r.decls.get(k)!.split(',').map(part => `${r.selector} -> ${part.trim().split(/\s+/)[0]}`)));
    expect(used.length).toBeGreaterThan(0);
    expect(used.filter(u => !allowed.has(u.split(' -> ')[1]))).toEqual([]);
  });

  test('keyframes never change opacity or colour, except the decorative logo light', () => {
    const blocks = [...css.matchAll(/@keyframes\s+([\w-]+)\s*\{((?:[^{}]*\{[^{}]*\})*)\s*\}/g)];
    expect(blocks.map(b => b[1]).sort()).toEqual(['draw', 'lgsweep', 'pop', 'rise', 'spin']);
    for (const [, name, body] of blocks) {
      if (name === 'lgsweep') continue; // the logo's sweeping light: an aria-hidden SVG stroke, never text
      expect(body, `@keyframes ${name}`).not.toMatch(/(?:^|[{;\s])(opacity|color|background(?:-color)?|fill)\s*:/);
    }
  });
});

describe('hosts board', () => {
  test('the board scrolls inside itself, so wide clusters never overflow the page', () => {
    expect(css).toMatch(/\.board\s*{[^}]*overflow-x:\s*auto;/);
    expect(css).toMatch(/\.board-cols\s*{[^}]*min-inline-size:\s*calc\(var\(--cols/);
  });
});

describe('control edges (WCAG 2.2 SC 1.4.11)', () => {
  test('--edge clears 3:1 on every surface a control sits on, including the hover fill', () => {
    for (const surface of ['bg', 'tile', 'tile2', 'raise']) {
      expect(contrast(token('edge'), token(surface)), `--edge on --${surface}`).toBeGreaterThanOrEqual(3);
    }
    expect(contrast(token('edge'), '#1a1a1a')).toBeGreaterThanOrEqual(3);
  });

  test('PR B fields, chips, segmented groups, the toggle and the current host use --edge', () => {
    const block = (selector: string) => {
      const start = css.indexOf(`\n${selector} {`);
      expect(start, selector).toBeGreaterThan(-1);
      return css.slice(start, css.indexOf('}', start));
    };
    for (const selector of ['.search', '.seg', '.fchip', '.filter-name', '.terminal-input input', 'a.hp[aria-current]']) {
      expect(block(selector), selector).toMatch(/var\(--edge\)/);
    }
    expect(block('.switch-track')).toMatch(/box-shadow:\s*inset 0 0 0 1px var\(--edge\)/);
  });
});
