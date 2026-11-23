'use strict';
// Unit tests for the startup asset table, with real junction/symlink fixtures.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { loadAssetTable } = require('./assets.cjs');

// Directory junctions need no privilege on Windows; elsewhere this is a dir symlink.
const linkDir = (target, at) => fs.symlinkSync(target, at, 'junction');

function fixture() {
  // realpath: on macOS os.tmpdir() lives under the /var symlink.
  const base = fs.mkdtempSync(path.join(fs.realpathSync.native(os.tmpdir()), 'arx-assets-'));
  const dist = path.join(base, 'dist');
  const outside = path.join(base, 'outside');
  fs.mkdirSync(path.join(dist, 'assets'), { recursive: true });
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(dist, 'index.html'), '<!doctype html><title>Arxcovo</title>');
  fs.writeFileSync(path.join(dist, 'assets', 'app.js'), 'console.log(1)');
  fs.writeFileSync(path.join(dist, 'assets', 'app.js.map'), '{}');
  fs.writeFileSync(path.join(outside, 'secret.js'), 'secret');
  return { base, dist, outside, cleanup: () => fs.rmSync(base, { recursive: true, force: true }) };
}

test('serves the startup snapshot with content types; unserved types are left out', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const table = loadAssetTable(f.dist);
  assert.equal(table.size, 2);
  const index = table.get('/');
  assert.equal(index.type, 'text/html; charset=utf-8');
  assert.match(index.body.toString(), /Arxcovo/);
  assert.equal(table.get('/assets/app.js').body.toString(), 'console.log(1)');
  assert.equal(table.get('/assets/app.js.map'), null);
});

test('lookups refuse traversal, dot segments, backslashes, NUL and bad encodings', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const table = loadAssetTable(f.dist);
  for (const p of ['/../outside/secret.js', '/%2e%2e/outside/secret.js', '/assets/../index.html', '//index.html', '/./index.html', '/assets%5capp.js', '/index.html%00', '/%E0%A4%A', 'index.html', '/assets', '/missing.js']) {
    assert.equal(table.get(p), null, p);
  }
});

test('a file changed or swapped after startup is never served', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const table = loadAssetTable(f.dist);
  const target = path.join(f.dist, 'assets', 'app.js');
  fs.writeFileSync(target, 'changed after startup');
  assert.equal(table.get('/assets/app.js').body.toString(), 'console.log(1)');
  fs.rmSync(path.join(f.dist, 'assets'), { recursive: true });
  linkDir(f.outside, path.join(f.dist, 'assets'));
  assert.equal(table.get('/assets/app.js').body.toString(), 'console.log(1)');
  assert.equal(table.get('/assets/secret.js'), null);
  // Returned bytes are copies: mutating them does not change the table.
  table.get('/index.html').body.fill(0);
  assert.match(table.get('/index.html').body.toString(), /Arxcovo/);
});

test('a startup walk that hits a junction refuses to start', async t => {
  const f = fixture();
  t.after(f.cleanup);
  linkDir(f.outside, path.join(f.dist, 'escape'));
  assert.throws(() => loadAssetTable(f.dist), /link in asset tree: escape/);
});

test('a startup walk that hits a file symlink refuses to start', async t => {
  const f = fixture();
  t.after(f.cleanup);
  try {
    fs.symlinkSync(path.join(f.outside, 'secret.js'), path.join(f.dist, 'assets', 'link.js'), 'file');
  } catch (error) {
    if (error.code === 'EPERM') return t.skip('file symlinks need privilege on this Windows account');
    throw error;
  }
  assert.throws(() => loadAssetTable(f.dist), /link in asset tree: assets\/link\.js/);
});

test('a root that is a junction, has a junction ancestor, or is a file refuses to start', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const redirected = path.join(f.base, 'redirected-dist');
  linkDir(f.dist, redirected);
  assert.throws(() => loadAssetTable(redirected), /redirected/);
  const parentLink = path.join(f.base, 'parent-link');
  linkDir(f.base, parentLink);
  assert.throws(() => loadAssetTable(path.join(parentLink, 'dist')), /redirected/);
  fs.writeFileSync(path.join(f.base, 'file-root'), 'x');
  assert.throws(() => loadAssetTable(path.join(f.base, 'file-root')), /not a plain directory/);
});

test('a file larger than the per-file cap at startup refuses to start', async t => {
  const f = fixture();
  t.after(f.cleanup);
  fs.writeFileSync(path.join(f.dist, 'big.js'), 'y'.repeat(2048));
  assert.throws(() => loadAssetTable(f.dist, { limits: { maxFileBytes: 1024 } }), /larger than 1024 bytes: big\.js/);
});

test('a file growing during the startup read is refused by the bounded reader', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const target = path.join(f.dist, 'assets', 'app.js');
  // The hook runs after the size check passed, so only the read loop can refuse.
  const hooks = { afterStat: file => { if (file === target) fs.appendFileSync(target, 'x'.repeat(4096)); } };
  assert.throws(() => loadAssetTable(f.dist, { limits: { maxFileBytes: 1024 }, hooks }), /grew past 1024 bytes while being read: assets\/app\.js/);
});

test('file-count, total-byte and depth caps refuse to start', async t => {
  const f = fixture();
  t.after(f.cleanup);
  assert.throws(() => loadAssetTable(f.dist, { limits: { maxFiles: 1 } }), /more than 1 files/);
  assert.throws(() => loadAssetTable(f.dist, { limits: { maxTotalBytes: 20 } }), /assets exceed 20 bytes/);
  fs.mkdirSync(path.join(f.dist, 'a', 'b', 'c'), { recursive: true });
  fs.writeFileSync(path.join(f.dist, 'a', 'b', 'c', 'deep.js'), '1');
  assert.throws(() => loadAssetTable(f.dist, { limits: { maxDepth: 2 } }), /deeper than 2/);
});

// Swaps happen at the beforeOpen seam, while no handle is open: Windows
// refuses to rename a directory that has an open file inside it.
test('a directory swapped for a junction during the walk refuses to start', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const assets = path.join(f.dist, 'assets');
  fs.writeFileSync(path.join(f.outside, 'app.js'), 'attacker');
  const hooks = {
    beforeOpen: file => {
      if (file !== path.join(assets, 'app.js')) return;
      fs.renameSync(assets, path.join(f.base, 'assets-moved'));
      linkDir(f.outside, assets);
    },
  };
  assert.throws(() => loadAssetTable(f.dist, { hooks }), /changed during load/);
});

test('a root replaced during the walk refuses to start', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const hooks = {
    beforeOpen: file => {
      if (file !== path.join(f.dist, 'index.html')) return;
      fs.renameSync(f.dist, path.join(f.base, 'dist-moved'));
      fs.mkdirSync(f.dist);
      fs.writeFileSync(path.join(f.dist, 'index.html'), 'attacker');
    },
  };
  assert.throws(() => loadAssetTable(f.dist, { hooks }), /changed during load/);
});

// The next two fixtures keep every file's identity intact, so only the
// directory-chain guard or the final root guard can refuse them.
test('an ancestor swapped for a junction to itself during the walk is caught by the directory-chain guard', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const assets = path.join(f.dist, 'assets');
  const hooks = {
    beforeOpen: file => {
      if (file !== path.join(assets, 'app.js')) return;
      // Same directory object, now reached through a junction: the file
      // opened next is the very file that was walked.
      const moved = path.join(f.base, 'assets-moved');
      fs.renameSync(assets, moved);
      linkDir(moved, assets);
    },
  };
  assert.throws(() => loadAssetTable(f.dist, { hooks }), /directory changed during load: .*assets/);
});

test('a root replaced after the last file is caught by the final root guard', async t => {
  for (const replace of ['copy', 'junction']) {
    const f = fixture();
    t.after(f.cleanup);
    const hooks = {
      beforeFinalCheck: () => {
        const moved = path.join(f.base, 'dist-moved');
        fs.renameSync(f.dist, moved);
        if (replace === 'copy') fs.cpSync(moved, f.dist, { recursive: true });
        else linkDir(moved, f.dist);
      },
    };
    assert.throws(() => loadAssetTable(f.dist, { hooks }), /asset root changed during load/, replace);
  }
});

test('every enumerated entry counts against the entry cap, including ignored ones', async t => {
  const f = fixture();
  t.after(f.cleanup);
  const ignored = path.join(f.dist, 'ignored');
  fs.mkdirSync(ignored);
  for (let i = 0; i < 10; i += 1) fs.writeFileSync(path.join(ignored, `note-${i}.txt`), 'x');
  // 2 root entries + 1 directory + 2 in assets + 10 ignored files = 15.
  assert.equal(loadAssetTable(f.dist, { limits: { maxEntries: 15 } }).size, 2);
  assert.throws(() => loadAssetTable(f.dist, { limits: { maxEntries: 14 } }), /more than 14 directory entries/);
});
