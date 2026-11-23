import { defineConfig, type Plugin } from 'vite';

// The stand-in adapter and its fixtures exist only for the dev server and
// Vitest. `vite build` sets __ARX_STANDIN__ to false in every mode and for
// every NODE_ENV, so the branch that imports them is compiled out; the guard
// below fails the build if fixture code reaches the bundle anyway.
const FORBIDDEN_MODULES = [/\/src\/api\/standin\.ts$/, /\/src\/api\/fixtures\.ts$/];
const FORBIDDEN_TEXT = ['arx-standin-fixture-v1', 'arxcovo-demo'];

export function standinGuard(): Plugin {
  return {
    name: 'arx-standin-guard',
    apply: 'build',
    generateBundle(_options, bundle) {
      for (const [file, output] of Object.entries(bundle)) {
        if (output.type !== 'chunk') continue;
        const modules = Object.keys(output.modules).map(id => id.replace(/\\/g, '/'));
        const module = modules.find(id => FORBIDDEN_MODULES.some(re => re.test(id)));
        if (module) this.error(`stand-in module ${module} reached the production bundle (${file})`);
        const text = FORBIDDEN_TEXT.find(t => output.code.includes(t));
        if (text) this.error(`stand-in data "${text}" reached the production bundle (${file})`);
      }
    },
  };
}

export default defineConfig(({ command }) => ({
  base: './',
  define: { __ARX_STANDIN__: JSON.stringify(command === 'serve') },
  plugins: [standinGuard()],
}));
