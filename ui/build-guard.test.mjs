// Proves deployable bundles never contain the stand-in adapter, whatever the
// build mode or NODE_ENV, and that the build fails if fixtures leak in.
import assert from 'node:assert/strict';
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';

const root = fileURLToPath(new URL('.', import.meta.url));
const MARKERS = ['arx-standin-fixture-v1', 'arxcovo-demo', 'claude-code-1'];

async function buildTo(overrides = {}) {
  const outDir = await mkdtemp(join(tmpdir(), 'arx-ui-guard-'));
  try {
    await build({ root, configFile: join(root, 'vite.config.ts'), logLevel: 'silent', build: { outDir, emptyOutDir: true }, ...overrides });
    const files = await readdir(join(outDir, 'assets'));
    const js = await Promise.all(files.filter(f => f.endsWith('.js')).map(f => readFile(join(outDir, 'assets', f), 'utf8')));
    return { files, js };
  } finally {
    await rm(outDir, { recursive: true, force: true });
  }
}

for (const [mode, nodeEnv] of [['production', 'development'], ['production', 'production'], ['development', 'development']]) {
  test(`vite build --mode ${mode} with NODE_ENV=${nodeEnv} ships no stand-in`, async () => {
    const previous = process.env.NODE_ENV;
    process.env.NODE_ENV = nodeEnv;
    try {
      const { files, js } = await buildTo({ mode });
      assert.ok(js.length > 0);
      assert.equal(files.some(f => f.includes('standin') || f.includes('fixtures')), false, files.join(', '));
      for (const code of js) for (const marker of MARKERS) assert.equal(code.includes(marker), false, `bundle contains ${marker}`);
    } finally {
      if (previous === undefined) delete process.env.NODE_ENV;
      else process.env.NODE_ENV = previous;
    }
  });
}

test('the build fails if the stand-in is forced into the bundle', async () => {
  await assert.rejects(buildTo({ define: { __ARX_STANDIN__: 'true' } }), /stand-in (module|data)/);
});
