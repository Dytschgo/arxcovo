import { HttpApi } from './http';
import type { AdapterKind, ArxApi } from './types';

export interface BuildEnv {
  DEV: boolean;
  MODE: string;
  VITE_ARX_API?: string;
}

/**
 * Production builds always talk to arxd. Dev and test builds use the stand-in
 * unless VITE_ARX_API=http asks for the real server.
 */
export function chooseAdapter(env: BuildEnv): AdapterKind {
  if (!env.DEV && env.MODE !== 'test') return 'http';
  return env.VITE_ARX_API === 'http' ? 'http' : 'standin';
}

export async function createApi(): Promise<ArxApi> {
  // `import.meta.env.DEV` is a build-time constant: production bundles drop
  // this branch and with it the stand-in module and its fixtures.
  if (import.meta.env.DEV) {
    if (chooseAdapter({ DEV: true, MODE: import.meta.env.MODE, VITE_ARX_API: import.meta.env.VITE_ARX_API as string | undefined }) === 'standin') {
      const { StandinApi } = await import('./standin');
      return new StandinApi({ latencyMs: 120, streamMs: 60 });
    }
  }
  return new HttpApi();
}
