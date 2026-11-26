// The colour maths behind the rendered contrast audit (ui/tests/contrast.ts).
// The audit injects this same factory into the page, so these numbers are the
// ones the smoke and live suites act on.
import { describe, expect, test } from 'vitest';
import { compositing, type Layer, type Rgba } from '../tests/compositing';

const C = compositing();
const grey = (v: number, a = 1): Rgba => [v, v, v, a];
const none = grey(0, 0);

describe('group opacity compositing', () => {
  // Hand computation (8-bit channel values, the page is opaque black):
  //   leaf box: no fill, opacity .5, white text.
  //     glyph = white;                     beside = nothing.
  //     group fade .5 -> glyph white at alpha .5; beside still nothing.
  //   parent box: fill #808080 (128), opacity .5.
  //     glyph  = .5·255 + .5·128 = 191.5 (opaque); beside = 128 (opaque).
  //     group fade .5 -> both at alpha .5.
  //   page black:
  //     glyph  = .5·191.5 = 95.75;          beside = .5·128 = 64.
  //   L(95.75) = ((95.75/255 + .055)/1.055)^2.4 = 0.11633
  //   L(64)    = ((64/255 + .055)/1.055)^2.4    = 0.05127
  //   ratio    = (0.11633 + .05) / (0.05127 + .05) = 1.6425
  // Fading text and fill separately (the old audit) puts the faded grey under
  // a .25-alpha white glyph: .25·255 + .75·64 = 111.75 on 64, 2.09:1.
  test('nested opacity groups fade each painted subtree once, level by level', () => {
    const layers: Layer[] = [
      { background: none, opacity: 0.5 },
      { background: grey(128), opacity: 0.5 },
    ];
    const { ink, background } = C.composite(grey(255), layers);
    expect(ink[0]).toBeCloseTo(95.75, 10);
    expect(ink[3]).toBe(1);
    expect(background[0]).toBeCloseTo(64, 10);
    expect(C.ratio(ink, background)).toBeCloseTo(1.6425, 4);
    expect(C.ratio(ink, background)).toBeLessThan(2.09);
  });

  // Re-review 3's mutation: `opacity: .89` on the login card (--tile #070707
  // on the black page) with the disabled "Signing in…" button (--ink4 #858585,
  // no fill). Glyph .89·133 = 118.37, beside .89·7 = 6.23: 4.478:1, under 4.5.
  // Chromium painted 118 on 6 (4.46:1). The old maths said 4.52:1 and passed.
  test('the reviewer’s card-opacity mutation lands under 4.5:1, as Chromium paints it', () => {
    const layers: Layer[] = [
      { background: none, opacity: 1 }, // button.button-primary:disabled
      { background: grey(7), opacity: 0.89 }, // form.login-card
      { background: none, opacity: 1 }, // div.login
      { background: grey(0), opacity: 1 }, // body
    ];
    const { ink, background } = C.composite(grey(133), layers);
    expect(ink[0]).toBeCloseTo(118.37, 10);
    expect(background[0]).toBeCloseTo(6.23, 10);
    expect(C.ratio(ink, background)).toBeCloseTo(4.4783, 4);
    expect(C.ratio(ink, background)).toBeLessThan(4.5);
  });

  test('without opacity groups it is plain source-over down to the page', () => {
    const { ink, background } = C.composite(grey(255, 0.5), [{ background: grey(0, 0.5), opacity: 1 }], grey(200));
    expect(background[0]).toBeCloseTo(100, 10);
    expect(ink[0]).toBeCloseTo(177.5, 10);
    expect(C.composite(grey(255), [{ background: grey(0), opacity: 1 }]).ink).toEqual(grey(255));
    expect(C.ratio(grey(255), grey(0))).toBe(21);
  });

  test('an opaque fill inside a faded group still lets the backdrop through', () => {
    // White text on an opaque black chip at opacity 0, over a white parent: the chip and its text vanish.
    const { ink, background } = C.composite(grey(255), [{ background: grey(0), opacity: 0 }, { background: grey(255), opacity: 1 }]);
    expect(ink).toEqual(grey(255));
    expect(background).toEqual(grey(255));
    expect(C.ratio(ink, background)).toBe(1);
  });

  test('parses the computed colour forms Chromium reports', () => {
    expect(C.parse('rgb(1, 2, 3)')).toEqual([1, 2, 3, 1]);
    expect(C.parse('rgba(1, 2, 3, 0.5)')).toEqual([1, 2, 3, 0.5]);
    expect(C.parse('transparent')).toEqual([0, 0, 0, 0]);
    expect(C.parse('color(srgb 1 0 0.5 / 0.25)')).toEqual([255, 0, 127.5, 0.25]);
    expect(() => C.parse('oklch(0.5 0 0)')).toThrow(/unparsed/);
    expect(C.hex([133, 133, 133, 1])).toBe('#858585');
  });

  test('the factory is self-contained, so its source runs in the page', () => {
    // The audit evaluates `(${compositing.toString()})()` in Chromium; any
    // reference to module scope would throw there.
    const rebuilt = new Function(`return (${compositing.toString()})();`)() as typeof C;
    expect(rebuilt.ratio(grey(255), grey(0))).toBe(21);
  });
});
