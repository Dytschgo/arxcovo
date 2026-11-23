'use strict';
// Unit tests for the app:// asset resolver with real junction/symlink fixtures.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { createAssetRoot, readAsset } = require('./assets.cjs');

// Directory junctions need no privilege on Windows; elsewhere this is a dir symlink.
const linkDir = (target, at) => fs.symlinkSync(target, at, 'junction');

function fixture() {
  // realpath: on macOS os.tmpdir() lives under the /var symlink.
  const base = fs.mkdtempSync(path.join(fs.realpathSync.native(os.tmpdir()), 'arx-assets-'));
  const dist = path.join(base, 'dist');
  const outside = path.join(base, 'outside');
  fs.mkdirSync(path.join(dist, 'assets'), { recursive: true });
  fs.mkdirSync(path.join(outside, 'assets'), { recursive: true });
  fs.writeFileSync(path.join(dist, 'index.html'), '<!doctype html><title>Arxcovo</title>');
  fs.writeFileSync(path.join(dist, 'assets', 'app.js'), 'console.log(1)');
  fs.writeFileSync(path.join(outside, 'secret.js'), 'secret');
  fs.writeFileSync(path.join(outside, 'assets', 'app.js'), 'attacker');
  return { base, dist, outside, cleanup: () => fs.rmSync(base, { recursive: true, force: true }) };
}

test('serves regular files under the root with their content type', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const root = createAssetRoot(f.dist);
  const index = await readAsset(root, '/');
  assert.equal(index.type, 'text/html; charset=utf-8');
  assert.match(index.body.toString(), /Arxcovo/);
  assert.equal((await readAsset(root, '/assets/app.js')).body.toString(), 'console.log(1)');
});

test('refuses traversal, encoded traversal, backslashes, NUL, unknown types and directories', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const root = createAssetRoot(f.dist);
  for (const p of ['/../outside/secret.js', '/%2e%2e/outside/secret.js', '/..%2foutside%2fsecret.js', '/assets%5c..%5c..%5coutside%5csecret.js', '/index.html%00.js', '/index.html.map', '/assets', '/%E0%A4%A']) {
    assert.equal(await readAsset(root, p), null, p);
  }
});

test('a root that is itself a junction or symlink, or a file, is rejected at startup', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const redirected = path.join(f.base, 'redirected-dist');
  linkDir(f.dist, redirected);
  assert.throws(() => createAssetRoot(redirected), /redirected/);
  fs.writeFileSync(path.join(f.base, 'file-root'), 'x');
  assert.throws(() => createAssetRoot(path.join(f.base, 'file-root')), /not a plain directory/);
});

test('a junction on an ancestor of the root is rejected at startup', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const parentLink = path.join(f.base, 'parent-link');
  linkDir(f.base, parentLink);
  assert.throws(() => createAssetRoot(path.join(parentLink, 'dist')), /redirected/);
});

test('an ancestor swapped for a junction after startup serves nothing', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const app = path.join(f.base, 'app');
  fs.mkdirSync(app);
  fs.renameSync(f.dist, path.join(app, 'dist'));
  const root = createAssetRoot(path.join(app, 'dist'));
  // Replace the ancestor with a junction to a look-alike tree.
  const lookalike = path.join(f.base, 'lookalike');
  fs.mkdirSync(path.join(lookalike, 'dist', 'assets'), { recursive: true });
  fs.writeFileSync(path.join(lookalike, 'dist', 'index.html'), 'attacker');
  fs.renameSync(app, path.join(f.base, 'app-moved'));
  linkDir(lookalike, app);
  assert.equal(await readAsset(root, '/index.html'), null);
});

test('a root replaced by a junction after startup serves nothing', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const root = createAssetRoot(f.dist);
  fs.renameSync(f.dist, path.join(f.base, 'moved'));
  fs.writeFileSync(path.join(f.outside, 'index.html'), 'attacker');
  linkDir(f.outside, f.dist);
  assert.equal(await readAsset(root, '/index.html'), null);
});

test('a junction inside the root cannot reach files outside it', async t => {
  const f = fixture();
  t.after(f.cleanup);
  linkDir(f.outside, path.join(f.dist, 'escape'));
  const root = createAssetRoot(f.dist);
  assert.equal(await readAsset(root, '/escape/secret.js'), null);
});

test('a directory swapped for a junction just before the open is refused after it', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const root = createAssetRoot(f.dist);
  const result = await readAsset(root, '/assets/app.js', {
    beforeOpen: () => {
      fs.renameSync(path.join(f.dist, 'assets'), path.join(f.base, 'assets-moved'));
      linkDir(path.join(f.outside, 'assets'), path.join(f.dist, 'assets'));
    },
  });
  assert.equal(result, null);
});

test('a file replaced between the open and the checks is refused (identity mismatch)', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const root = createAssetRoot(f.dist);
  const target = path.join(f.dist, 'assets', 'app.js');
  const result = await readAsset(root, '/assets/app.js', {
    afterOpen: () => {
      fs.renameSync(target, path.join(f.base, 'opened-file.js'));
      fs.writeFileSync(target, 'replacement');
    },
  });
  assert.equal(result, null);
});

test('the byte cap counts bytes read, so a file growing after the size check is refused', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const root = createAssetRoot(f.dist, { maxBytes: 1024 });
  assert.ok(await readAsset(root, '/assets/app.js'));
  const target = path.join(f.dist, 'assets', 'app.js');
  const result = await readAsset(root, '/assets/app.js', { afterOpen: () => fs.appendFileSync(target, 'x'.repeat(4096)) });
  assert.equal(result, null);
  fs.writeFileSync(path.join(f.dist, 'big.js'), 'y'.repeat(2048));
  assert.equal(await readAsset(root, '/big.js'), null);
});

test('a file symlink inside the root is refused', async t => {
  const f = fixture();
  t.after(f.cleanup);
  try {
    fs.symlinkSync(path.join(f.outside, 'secret.js'), path.join(f.dist, 'assets', 'link.js'), 'file');
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('file symlinks need privilege on this Windows account');
    throw error;
  }
  const root = createAssetRoot(f.dist);
  assert.equal(await readAsset(root, '/assets/link.js'), null);
});
