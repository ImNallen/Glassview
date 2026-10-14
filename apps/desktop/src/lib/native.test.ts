import { expect, it, vi } from 'vitest';
import { onOverlay, type OverlayEvent } from './native';

type Listen = (name: string, handler: (event: { payload: OverlayEvent }) => void) => Promise<() => void>;
const listen = vi.fn<Listen>(async () => () => {});
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => ({ label: 'overlay-2', listen }) }));

it('listens on this overlay window label, so Tauri evaluates nothing in the other overlays', async () => {
  const seen: OverlayEvent[] = [];
  await onOverlay(event => seen.push(event));
  expect(listen).toHaveBeenCalledTimes(1);
  const [name, handler] = listen.mock.calls[0];
  expect(name).toBe('overlay-2');
  handler({ payload: { kind: 'halo', at: null } });
  expect(seen).toEqual([{ kind: 'halo', at: null }]);
});
