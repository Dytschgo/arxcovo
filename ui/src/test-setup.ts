import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// jsdom lacks object URLs; the console viewer only needs unique strings.
let nextUrl = 0;
if (!URL.createObjectURL) URL.createObjectURL = () => `blob:test/${++nextUrl}`;
if (!URL.revokeObjectURL) URL.revokeObjectURL = () => undefined;

afterEach(() => {
  cleanup();
  window.location.hash = '';
});
