'use strict';
/* global module */
// Immutable in-memory asset table for the app:// protocol.
//
// Trust boundary: ui/dist is install content. At startup, after validating
// and pinning the root (its real path equals the requested path, every
// ancestor is a plain directory, device and inode recorded), the loader walks
// the tree once, refusing any symlink, junction or other reparse point, and
// reads each served file through a verified handle with byte-bounded reads,
// under file-count, per-file and total-byte caps. It then re-verifies the
// root identity and refuses to start on any mismatch. Requests are answered
// from the frozen table only, so nothing on disk can change what is served
// after startup and no per-request filesystem access exists to race.
const fs = require('node:fs');
const path = require('node:path');

const TYPES = Object.freeze({
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.json': 'application/json',
});

// maxEntries counts every directory entry enumerated (files, directories and
// ignored entries alike), so enumeration itself is bounded, not only what is served.
const DEFAULT_LIMITS = Object.freeze({ maxFiles: 1000, maxEntries: 4000, maxFileBytes: 16 * 1024 * 1024, maxTotalBytes: 64 * 1024 * 1024, maxDepth: 16 });
// O_NOFOLLOW exists on POSIX; Windows has no such flag, so there the lstat
// and handle-identity checks are what refuse links.
const OPEN_FLAGS = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);

function samePath(a, b) {
  return process.platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b;
}

function sameObject(a, b) {
  return a.dev === b.dev && a.ino === b.ino;
}

function refuse(message) {
  throw new Error(`asset table refused: ${message}`);
}

/** Validates and pins the asset root; throws unless it and its ancestors are plain directories. */
function createAssetRoot(dir) {
  const requested = path.resolve(dir);
  const real = fs.realpathSync.native(requested);
  if (!samePath(real, requested)) refuse(`asset root is redirected: ${requested} resolves to ${real}`);
  for (let cursor = real; ; cursor = path.dirname(cursor)) {
    const stat = fs.lstatSync(cursor);
    if (stat.isSymbolicLink() || !stat.isDirectory()) refuse(`asset root is not a plain directory: ${cursor}`);
    if (path.dirname(cursor) === cursor) break;
  }
  const stat = fs.lstatSync(real, { bigint: true });
  return { root: real, dev: stat.dev, ino: stat.ino };
}

/** Reads at most `limit` bytes; returns null if the handle yields more. */
function readBounded(fd, limit) {
  const chunks = [];
  let total = 0;
  const buffer = Buffer.alloc(64 * 1024);
  for (;;) {
    const bytesRead = fs.readSync(fd, buffer, 0, buffer.length, null);
    if (bytesRead === 0) return Buffer.concat(chunks, total);
    total += bytesRead;
    if (total > limit) return null;
    chunks.push(Buffer.from(buffer.subarray(0, bytesRead)));
  }
}

/**
 * Loads ui/dist into a frozen table. Throws (the app refuses to start) on a
 * link, a cap overrun, a file that changes identity or grows while being
 * read, too many entries, or a root that changed during the walk. Test seams: `hooks.beforeOpen(file)`
 * runs after a file's lstat and before its open; `hooks.afterStat(file)` runs
 * after its size check and before its read; `hooks.beforeFinalCheck()` runs
 * after the walk and before the final root re-verification.
 */
function loadAssetTable(dir, options = {}) {
  const limits = { ...DEFAULT_LIMITS, ...options.limits };
  const hooks = options.hooks ?? {};
  const assetRoot = createAssetRoot(dir);
  const entries = new Map();
  let files = 0;
  let enumerated = 0;
  let totalBytes = 0;

  // `chain` holds the lstat of every directory from the root down to `dirPath`.
  function verifyChain(chain) {
    for (const { at, stat } of chain) {
      const now = fs.lstatSync(at, { bigint: true });
      if (now.isSymbolicLink() || !now.isDirectory() || !sameObject(now, stat)) refuse(`directory changed during load: ${at}`);
    }
  }

  function walk(dirPath, rel, chain) {
    if (chain.length > limits.maxDepth) refuse(`tree deeper than ${limits.maxDepth}`);
    // Batched enumeration against the entry budget: refuses before an
    // oversized directory is ever listed in full.
    const names = [];
    const dir = fs.opendirSync(dirPath, { bufferSize: 64 });
    try {
      for (let entry = dir.readSync(); entry !== null; entry = dir.readSync()) {
        if (++enumerated > limits.maxEntries) refuse(`more than ${limits.maxEntries} directory entries`);
        names.push(entry.name);
      }
    } finally {
      dir.closeSync();
    }
    for (const name of names.sort()) {
      const full = path.join(dirPath, name);
      const key = rel ? `${rel}/${name}` : name;
      const stat = fs.lstatSync(full, { bigint: true });
      // Known limitation (deferred, recorded as a known issue): Node classifies
      // symlinks and junctions as links, but on Windows other reparse tags
      // (for example cloud-file placeholders or dedup) can be reported as
      // ordinary files or directories, because libuv retries them as a plain
      // stat. Rejecting every FILE_ATTRIBUTE_REPARSE_POINT needs a native
      // attribute query that Node does not expose.
      if (stat.isSymbolicLink()) refuse(`link in asset tree: ${key}`);
      if (stat.isDirectory()) {
        walk(full, key, [...chain, { at: full, stat }]);
        continue;
      }
      if (!stat.isFile()) refuse(`not a regular file: ${key}`);
      const type = TYPES[path.extname(name).toLowerCase()];
      if (!type) continue; // not served (for example source maps)
      if (++files > limits.maxFiles) refuse(`more than ${limits.maxFiles} files`);
      if (hooks.beforeOpen) hooks.beforeOpen(full);
      const fd = fs.openSync(full, OPEN_FLAGS);
      try {
        const opened = fs.fstatSync(fd, { bigint: true });
        if (!opened.isFile() || !sameObject(opened, stat)) refuse(`file changed during load: ${key}`);
        if (opened.size > BigInt(limits.maxFileBytes)) refuse(`file larger than ${limits.maxFileBytes} bytes: ${key}`);
        if (hooks.afterStat) hooks.afterStat(full);
        const body = readBounded(fd, limits.maxFileBytes);
        if (!body) refuse(`file grew past ${limits.maxFileBytes} bytes while being read: ${key}`);
        totalBytes += body.length;
        if (totalBytes > limits.maxTotalBytes) refuse(`assets exceed ${limits.maxTotalBytes} bytes`);
        // The directories leading here must still be the ones walked.
        verifyChain(chain);
        entries.set(key, Object.freeze({ body, type }));
      } finally {
        fs.closeSync(fd);
      }
    }
  }

  walk(assetRoot.root, '', [{ at: assetRoot.root, stat: fs.lstatSync(assetRoot.root, { bigint: true }) }]);
  if (hooks.beforeFinalCheck) hooks.beforeFinalCheck();
  const end = fs.lstatSync(assetRoot.root, { bigint: true });
  if (end.isSymbolicLink() || end.dev !== assetRoot.dev || end.ino !== assetRoot.ino || !samePath(fs.realpathSync.native(assetRoot.root), assetRoot.root)) {
    refuse('asset root changed during load');
  }
  if (!entries.has('index.html')) refuse('index.html missing');

  return Object.freeze({
    size: entries.size,
    /** Looks up a URL pathname; returns a copy of the bytes and the type, or null. */
    get(pathname) {
      let rel;
      try {
        rel = decodeURIComponent(pathname);
      } catch {
        return null;
      }
      if (!rel.startsWith('/') || rel.includes('\0') || rel.includes('\\')) return null;
      if (rel === '/') rel = '/index.html';
      const normalized = path.posix.normalize(rel);
      if (normalized !== rel) return null; // no dot segments or doubled slashes
      const entry = entries.get(normalized.slice(1));
      return entry ? { body: Buffer.from(entry.body), type: entry.type } : null;
    },
  });
}

module.exports = { createAssetRoot, loadAssetTable, TYPES, DEFAULT_LIMITS };
