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
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'arx-assets-'));
  const dist = path.join(base, 'dist');
  const outside = path.join(base, 'outside');
  fs.mkdirSync(path.join(dist, 'assets'), { recursive: true });
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(dist, 'index.html'), '<!doctype html><title>Arxcovo</title>');
  fs.writeFileSync(path.join(dist, 'assets', 'app.js'), 'console.log(1)');
  fs.writeFileSync(path.join(outside, 'secret.js'), 'secret');
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

test('a root that is itself a junction or symlink is rejected at startup', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const redirected = path.join(f.base, 'redirected-dist');
  linkDir(f.dist, redirected);
  assert.throws(() => createAssetRoot(redirected), /not a plain directory/);
  fs.writeFileSync(path.join(f.base, 'file-root'), 'x');
  assert.throws(() => createAssetRoot(path.join(f.base, 'file-root')), /not a plain directory/);
});

test('a root replaced by a junction after startup serves nothing', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const root = createAssetRoot(f.dist);
  fs.renameSync(f.dist, path.join(f.base, 'moved'));
  fs.mkdirSync(path.join(f.outside, 'assets'));
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
