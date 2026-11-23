import { HttpApi } from './http';
import type { AdapterKind, ArxApi } from './types';

export interface BuildFlags {
  /** __ARX_STANDIN__: true only for the Vite dev server and Vitest. */
  standinAllowed: boolean;
  VITE_ARX_API?: string;
}

/**
 * Bundles produced by `vite build` always talk to arxd. The dev server and
 * tests use the stand-in unless VITE_ARX_API=http asks for the real server.
 */
export function chooseAdapter(flags: BuildFlags): AdapterKind {
  if (!flags.standinAllowed) return 'http';
  return flags.VITE_ARX_API === 'http' ? 'http' : 'standin';
}

export async function createApi(): Promise<ArxApi> {
  // __ARX_STANDIN__ is a build-time constant (ui/vite.config.ts), false for
  // every `vite build` regardless of mode or NODE_ENV, so this branch and the
  // stand-in module are compiled out of deployable bundles.
  if (__ARX_STANDIN__) {
    if (chooseAdapter({ standinAllowed: true, VITE_ARX_API: import.meta.env.VITE_ARX_API as string | undefined }) === 'standin') {
      const { StandinApi } = await import('./standin');
      return new StandinApi({ latencyMs: 120, streamMs: 60 });
    }
  }
  return new HttpApi();
}
