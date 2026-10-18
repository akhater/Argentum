import { renderToStaticMarkup } from 'react-dom/server';

import type { ToolDescriptor } from '../contract';
import type { EngineClient } from '../engine/client';
import { createFakeEngine, FAKE_TOOLS, type FakeEngine } from '../engine/fake';
import { PanelStack } from '../panels/PanelStack';
import { ShellProvider } from '../ShellContext';
import { createShellStore, type ShellStore } from '../store';

/** Let every pending timer and promise run: the fake answers on the next tick. */
export async function settle() {
  for (let i = 0; i < 8; i++) await new Promise((r) => setTimeout(r, 0));
}

/** A store on the contract fake, with one photo open and a view of 800 x 600. */
export async function openShell(
  options: { tools?: ToolDescriptor[]; client?: (engine: FakeEngine) => EngineClient } = {},
): Promise<{ engine: FakeEngine; store: ShellStore }> {
  const engine = createFakeEngine({ tools: options.tools ?? FAKE_TOOLS });
  const store = createShellStore(options.client ? options.client(engine) : engine, { photos: ['samples/A.CR3'] });
  await store.getState().init();
  store.getState().setViewSize({ width_px: 800, height_px: 600 });
  await settle();
  return { engine, store };
}

export const renderPanels = (store: ShellStore) =>
  renderToStaticMarkup(
    <ShellProvider store={store}>
      <PanelStack />
    </ShellProvider>,
  );

export const applies = (engine: FakeEngine) => engine.calls.flatMap((c) => (c.method === 'apply' ? [c.patch] : []));
