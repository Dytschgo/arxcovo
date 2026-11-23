'use strict';
/* global Response */
// Arxcovo desktop shell. Serves the built dashboard (ui/dist) from a private
// app://arxcovo origin with a strict CSP, sandboxed renderers, no Node in the
// page, no permissions, no popups and navigation locked to that origin.
const { app, BrowserWindow, protocol, session } = require('electron');
const path = require('node:path');
const { loadAssetTable } = require('./assets.cjs');

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

function respond(status, body, type) {
  return new Response(body, { status, headers: { ...BASE_HEADERS, 'Content-Type': type } });
}

function handle(assets, request) {
  if (!isAppUrl(request.url)) return respond(404, 'Not found', 'text/plain');
  const url = new URL(request.url);
  if (url.pathname.startsWith('/api/')) {
    // Host connection is not built yet; every method, including sign-in,
    // gets the documented 503 instead of a generic error.
    return respond(503, JSON.stringify({ error: { code: 'unavailable', message: 'This desktop build is not connected to an arxd host yet.' } }), 'application/json');
  }
  if (request.method !== 'GET' && request.method !== 'HEAD') return respond(405, 'Method not allowed', 'text/plain');
  // Served from the startup snapshot only: no filesystem access per request.
  const asset = assets.get(url.pathname);
  if (!asset) return respond(404, 'Not found', 'text/plain');
  return respond(200, asset.body, asset.type);
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
  let assets;
  try {
    assets = loadAssetTable(DIST);
  } catch (error) {
    console.error(`Arxcovo cannot start: ${error.message}`);
    app.exit(1);
    return;
  }
  const ses = session.defaultSession;
  ses.setPermissionRequestHandler((_wc, _permission, callback) => callback(false));
  ses.setPermissionCheckHandler(() => false);
  ses.webRequest.onBeforeRequest({ urls: ['http://*/*', 'https://*/*', 'ws://*/*', 'wss://*/*'] }, (_details, callback) => callback({ cancel: true }));
  protocol.handle(SCHEME, request => handle(assets, request));

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
