// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import App from './App.svelte';
import { DEFAULT_SESSION, settingsReady, subscribe, type Session } from './lib/native';

vi.mock('./lib/native', async original => ({
  ...await original<typeof import('./lib/native')>(),
  subscribe: vi.fn(),
  settingsReady: vi.fn(() => Promise.resolve()),
  onOverlay: () => Promise.resolve(() => {}),
}));

let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  document.body.replaceChildren();
  vi.resetAllMocks();
});

function open(surface: string, session: Session) {
  history.replaceState(null, '', `/?surface=${surface}`);
  let deliver = () => {};
  vi.mocked(subscribe).mockImplementation(fn => new Promise(resolve => {
    deliver = () => { fn(session); resolve(() => {}); };
  }));
  app = mount(App, { target: document.body });
  flushSync();
  deliver();
}

it('tells Rust the settings page is ready only after it rendered the real session', async () => {
  let shown = '';
  vi.mocked(settingsReady).mockImplementation(() => { shown = document.body.textContent ?? ''; return Promise.resolve(); });
  open('settings', { ...structuredClone(DEFAULT_SESSION), shortcut: 'Ctrl+Alt+Shift+K' });
  await vi.waitFor(() => expect(settingsReady).toHaveBeenCalledOnce());
  expect(shown).toContain('Toggle anytime with Ctrl+Alt+Shift+K');
});

it('never reports an overlay as ready', async () => {
  open('overlay', structuredClone(DEFAULT_SESSION));
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(settingsReady).not.toHaveBeenCalled();
});
