'use strict';
/* global module */
// Confined, link-free reads of the built dashboard for the app:// protocol.
// The asset root must be a plain directory (not a symlink or junction) and
// every request must resolve, component by component, to a regular file
// under it. Reads go through a handle whose identity matches the validated
// file, so a swap between validation and read is refused.
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

function samePath(a, b) {
  return process.platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b;
}

function inside(root, candidate) {
  const rel = path.relative(root, candidate);
  return rel !== '' && !rel.startsWith('..') && !path.isAbsolute(rel);
}

/** Validates the asset root once at startup; throws if it is not a plain directory. */
function createAssetRoot(dir) {
  const root = path.resolve(dir);
  const stat = fs.lstatSync(root);
  if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`asset root is not a plain directory: ${root}`);
  return { root, real: fs.realpathSync.native(root) };
}

/** Maps a URL pathname to a validated file path and its lstat, or null. */
function resolveAsset(assetRoot, pathname) {
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
  if (!Object.prototype.hasOwnProperty.call(TYPES, path.extname(file).toLowerCase())) return null;
  let fileStat;
  try {
    // Every component from the file up to and including the root: no links.
    for (let cursor = file; ; cursor = path.dirname(cursor)) {
      const stat = fs.lstatSync(cursor, { bigint: true });
      if (stat.isSymbolicLink()) return null;
      if (cursor === file) {
        if (!stat.isFile()) return null;
        fileStat = stat;
      }
      if (cursor === assetRoot.root) {
        if (!stat.isDirectory()) return null;
        break;
      }
      if (path.dirname(cursor) === cursor) return null;
    }
    // The backing path must still be the validated root's.
    if (!samePath(fs.realpathSync.native(assetRoot.root), assetRoot.real)) return null;
    const real = fs.realpathSync.native(file);
    if (!inside(assetRoot.real, real)) return null;
  } catch {
    return null;
  }
  return { file, stat: fileStat, type: TYPES[path.extname(file).toLowerCase()] };
}

/** Reads a validated asset through a handle bound to the validated file. */
async function readAsset(assetRoot, pathname) {
  const resolved = resolveAsset(assetRoot, pathname);
  if (!resolved) return null;
  let handle;
  try {
    handle = await fs.promises.open(resolved.file, fs.constants.O_RDONLY);
    const opened = await handle.stat({ bigint: true });
    if (!opened.isFile() || opened.ino !== resolved.stat.ino || opened.dev !== resolved.stat.dev) return null;
    if (opened.size > BigInt(MAX_ASSET_BYTES)) return null;
    return { body: await handle.readFile(), type: resolved.type };
  } catch {
    return null;
  } finally {
    await handle?.close();
  }
}

module.exports = { createAssetRoot, resolveAsset, readAsset, TYPES };
