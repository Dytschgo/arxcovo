'use strict';
/* global module */
// Confined, link-free reads of the built dashboard for the app:// protocol.
//
// Trust boundary: the asset root (ui/dist) and every ancestor of it must be
// plain directories, and the root is pinned by device and inode at startup.
// Each request opens the candidate first and then proves, against the opened
// handle, that the object it will read is the regular file at the confined
// path: every component from the file up to the pinned root is re-checked
// after the open, and the handle's identity must match. The bytes come only
// from that handle, through a bounded read.
const fs = require('node:fs');
const path = require('node:path');

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.json': 'application/json',
};

const MAX_ASSET_BYTES = 16 * 1024 * 1024;
// O_NOFOLLOW exists on POSIX; Windows has no equivalent flag, so there the
// post-open lstat/realpath/identity checks are what refuse links.
const OPEN_FLAGS = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);

function samePath(a, b) {
  return process.platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b;
}

function inside(root, candidate) {
  const rel = path.relative(root, candidate);
  return rel !== '' && !rel.startsWith('..') && !path.isAbsolute(rel);
}

function sameObject(a, b) {
  return a.dev === b.dev && a.ino === b.ino;
}

/**
 * Validates and pins the asset root once at startup. Throws unless the path
 * resolves to itself (no symlink or junction anywhere in it) and every
 * component of the real path is a plain directory.
 */
function createAssetRoot(dir, { maxBytes = MAX_ASSET_BYTES } = {}) {
  const requested = path.resolve(dir);
  const real = fs.realpathSync.native(requested);
  if (!samePath(real, requested)) throw new Error(`asset root is redirected: ${requested} resolves to ${real}`);
  for (let cursor = real; ; cursor = path.dirname(cursor)) {
    const stat = fs.lstatSync(cursor);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`asset root is not a plain directory: ${cursor}`);
    if (path.dirname(cursor) === cursor) break;
  }
  const stat = fs.lstatSync(real, { bigint: true });
  return { root: real, dev: stat.dev, ino: stat.ino, maxBytes };
}

/** Maps a URL pathname to a candidate file path under the root, or null. */
function candidate(assetRoot, pathname) {
  let rel;
  try {
    rel = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  if (rel === '/') rel = '/index.html';
  if (!rel.startsWith('/') || rel.includes('\0') || rel.includes('\\')) return null;
  const file = path.resolve(assetRoot.root, `.${rel}`);
  if (!inside(assetRoot.root, file)) return null;
  const type = TYPES[path.extname(file).toLowerCase()];
  return type ? { file, type } : null;
}

/**
 * Proves, after the open, that `opened` (the handle's fstat) is the regular
 * file at `file` and that the path from it to the pinned root has no links.
 */
function confined(assetRoot, file, opened) {
  if (!opened.isFile()) return false;
  for (let cursor = file; ; cursor = path.dirname(cursor)) {
    const stat = fs.lstatSync(cursor, { bigint: true });
    if (stat.isSymbolicLink()) return false;
    if (cursor === file && !sameObject(stat, opened)) return false;
    if (cursor === assetRoot.root) {
      if (!stat.isDirectory() || stat.dev !== assetRoot.dev || stat.ino !== assetRoot.ino) return false;
      break;
    }
    if (cursor !== file && !stat.isDirectory()) return false;
    if (path.dirname(cursor) === cursor) return false;
  }
  if (!samePath(fs.realpathSync.native(file), file)) return false;
  if (!sameObject(fs.statSync(file, { bigint: true }), opened)) return false;
  return true;
}

async function readBounded(handle, limit) {
  const chunks = [];
  let total = 0;
  const buffer = Buffer.alloc(64 * 1024);
  for (;;) {
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, null);
    if (bytesRead === 0) return Buffer.concat(chunks, total);
    total += bytesRead;
    if (total > limit) return null;
    chunks.push(Buffer.from(buffer.subarray(0, bytesRead)));
  }
}

/**
 * Reads a confined asset. `hooks` exist only for tests that simulate a swap
 * at the validation boundary: `beforeOpen(file)` and `afterOpen(file)`.
 */
async function readAsset(assetRoot, pathname, hooks = {}) {
  const target = candidate(assetRoot, pathname);
  if (!target) return null;
  let handle;
  try {
    if (hooks.beforeOpen) await hooks.beforeOpen(target.file);
    handle = await fs.promises.open(target.file, OPEN_FLAGS);
    if (hooks.afterOpen) await hooks.afterOpen(target.file);
    const opened = await handle.stat({ bigint: true });
    if (opened.size > BigInt(assetRoot.maxBytes)) return null;
    if (!confined(assetRoot, target.file, opened)) return null;
    // Bounded by bytes actually read, so a file growing after fstat cannot exceed the cap.
    const body = await readBounded(handle, assetRoot.maxBytes);
    return body ? { body, type: target.type } : null;
  } catch {
    return null;
  } finally {
    await handle?.close();
  }
}

module.exports = { createAssetRoot, readAsset, TYPES, MAX_ASSET_BYTES };
