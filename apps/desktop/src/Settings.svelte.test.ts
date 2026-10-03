// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Settings from './Settings.svelte';
import { action, DEFAULT_PREFERENCES, DEFAULT_SESSION, savePreferences, type Session } from './lib/native';

vi.mock('./lib/native', async original => ({
  ...await original<typeof import('./lib/native')>(),
  action: vi.fn(() => Promise.resolve()),
  savePreferences: vi.fn(() => Promise.resolve()),
}));

let settings: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (settings) await unmount(settings);
  settings = undefined;
  vi.clearAllMocks();
  document.body.replaceChildren();
});

function open(patch: Partial<Session> = {}) {
  settings = mount(Settings, { target: document.body, props: { session: { ...structuredClone(DEFAULT_SESSION), ...patch } } });
  flushSync();
}
const button = (name: string) => [...document.querySelectorAll('button')].find(element => element.textContent?.trim() === name || element.getAttribute('aria-label') === name)!;

it('saves all keys with the rest of the preferences unchanged, and warns what it shows', () => {
  open();
  expect(document.body.textContent).toContain('Typed text, including passwords, is never shown.');
  button('All keys').click();
  expect(savePreferences).toHaveBeenCalledWith({ ...DEFAULT_PREFERENCES, keys: 'all' });
  open({ preferences: { ...DEFAULT_PREFERENCES, keys: 'all' } });
  expect(document.body.textContent).toContain('Everything you type appears on screen, including passwords.');
});

it('saves the pill position and the halo switch', () => {
  open();
  button('Top right').click();
  expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, pillPosition: 'top-right' });
  document.querySelector<HTMLButtonElement>('[aria-labelledby="halo-label"]')!.click();
  expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, halo: true });
});

it('asks for key access while it is undecided and offers System Settings once denied', () => {
  open({ keyAccess: 'unknown' });
  button('Allow…').click();
  expect(action).toHaveBeenCalledWith('request-key-access');
  unmount(settings!);
  open({ keyAccess: 'denied' });
  expect(button('Open System Settings')).toBeDefined();
  unmount(settings!);
  open({ keyAccess: 'granted' });
  expect(document.querySelector('#access-title')).toBeNull();
  unmount(settings!);
  open({ keyAccess: 'granted', grantedWhileRunning: true });
  button('Relaunch').click();
  expect(action).toHaveBeenCalledWith('relaunch');
});

it('toggles through the shared action and closes on Escape', () => {
  open({ enabled: true, shortcut: 'Ctrl+Alt+Shift+K' });
  expect(document.body.textContent).toContain('Toggle anytime with Ctrl+Alt+Shift+K');
  document.querySelector<HTMLButtonElement>('[aria-labelledby="enabled-label"]')!.click();
  expect(action).toHaveBeenCalledWith('toggle');
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true }));
  expect(action).toHaveBeenLastCalledWith('close-settings');
});
