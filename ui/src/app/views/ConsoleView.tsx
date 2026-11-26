import { VmPage } from './VmDetailView';

/** The console route: the VM detail page with its console tile streaming. */
export function ConsoleView({ id }: { id: string }) {
  return <VmPage id={id} console />;
}
