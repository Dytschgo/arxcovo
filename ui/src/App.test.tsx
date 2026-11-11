import { renderToStaticMarkup } from 'react-dom/server';
import { expect, test } from 'vitest';
import { App } from './App';

test('the scaffold identifies the product and does not claim host management exists', () => {
  const html = renderToStaticMarkup(<App />);
  expect(html).toContain('<main>');
  expect(html).toContain('<h1>Arxcovo</h1>');
  expect(html).toContain('Workspace scaffold. Host management arrives in milestone 2.');
});
