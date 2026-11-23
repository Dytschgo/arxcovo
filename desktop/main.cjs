'use strict';
/* global Response */
// Arxcovo desktop shell. Serves the built dashboard (ui/dist) from a private
// app://arxcovo origin with a strict CSP, sandboxed renderers, no Node in the
// page, no permissions, no popups and navigation locked to that origin.
const { app, BrowserWindow, protocol, session } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

const SCHEME = 'app';
const HOST = 'arxcovo';
const START_URL = `${SCHEME}://${HOST}/index.html`;
const DIST = path.resolve(__dirname, '../ui/dist');

const CSP = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "img-src 'self' blob: data:",
  "font-src 'self'",
  "connect-src 'self'",
  "base-uri 'none'",
  "form-action 'self'",
  "frame-ancestors 'none'",
  "object-src 'none'",
].join('; ');

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

const BASE_HEADERS = {
  'Content-Security-Policy': CSP,
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'no-referrer',
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cache-Control': 'no-store',
};

protocol.registerSchemesAsPrivileged([
  { scheme: SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true } },
]);
app.enableSandbox();

function isAppUrl(raw) {
  try {
    const url = new URL(raw);
    return url.protocol === `${SCHEME}:` && url.host === HOST;
  } catch {
    return false;
  }
}

/** Maps an app:// URL to a regular file inside ui/dist, or null. */
function resolveAsset(raw) {
  if (!isAppUrl(raw)) return null;
  let rel;
  try {
    rel = decodeURIComponent(new URL(raw).pathname);
  } catch {
    return null;
  }
  if (rel === '/') rel = '/index.html';
  if (rel.includes('\0') || rel.includes('\\')) return null;
  const file = path.resolve(DIST, `.${rel}`);
  if (!file.startsWith(DIST + path.sep)) return null;
  if (!Object.prototype.hasOwnProperty.call(TYPES, path.extname(file))) return null;
  try {
    // lstat on every component: refuse symlinks/junctions (reparse points).
    let cursor = file;
    while (cursor !== DIST) {
      const stat = fs.lstatSync(cursor);
      if (stat.isSymbolicLink()) return null;
      if (cursor === file && !stat.isFile()) return null;
      cursor = path.dirname(cursor);
    }
  } catch {
    return null;
  }
  return file;
}

function respond(status, body, type) {
  return new Response(body, { status, headers: { ...BASE_HEADERS, 'Content-Type': type } });
}

async function handle(request) {
  if (request.method !== 'GET' && request.method !== 'HEAD') return respond(405, 'Method not allowed', 'text/plain');
  const url = new URL(request.url);
  if (url.pathname.startsWith('/api/')) {
    // Host connection is not built yet; say so instead of pretending.
    return respond(503, JSON.stringify({ error: { code: 'unavailable', message: 'This desktop build is not connected to an arxd host yet.' } }), 'application/json');
  }
  const file = resolveAsset(request.url);
  if (!file) return respond(404, 'Not found', 'text/plain');
  return respond(200, await fs.promises.readFile(file), TYPES[path.extname(file)]);
}

function lockDown(contents) {
  contents.setWindowOpenHandler(() => ({ action: 'deny' }));
  const guard = (event, url) => {
    if (!isAppUrl(url)) event.preventDefault();
  };
  contents.on('will-navigate', guard);
  contents.on('will-redirect', guard);
  contents.on('will-attach-webview', event => event.preventDefault());
}

app.on('web-contents-created', (_event, contents) => lockDown(contents));

app.whenReady().then(() => {
  const ses = session.defaultSession;
  ses.setPermissionRequestHandler((_wc, _permission, callback) => callback(false));
  ses.setPermissionCheckHandler(() => false);
  ses.webRequest.onBeforeRequest({ urls: ['http://*/*', 'https://*/*', 'ws://*/*', 'wss://*/*'] }, (_details, callback) => callback({ cancel: true }));
  protocol.handle(SCHEME, handle);

  const window = new BrowserWindow({
    width: 1280,
    height: 820,
    title: 'Arxcovo',
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      webviewTag: false,
      navigateOnDragDrop: false,
      spellcheck: false,
    },
  });
  window.loadURL(START_URL);
});

app.on('window-all-closed', () => app.quit());

