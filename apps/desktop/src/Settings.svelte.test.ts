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
async function close() {
  if (settings) await unmount(settings);
  settings = undefined;
  document.body.replaceChildren();
}
afterEach(async () => {
  await close();
  vi.resetAllMocks();
  vi.restoreAllMocks();
});

async function open(patch: Partial<Session> = {}) {
  await close();
  settings = mount(Settings, { target: document.body, props: { session: { ...structuredClone(DEFAULT_SESSION), ...patch } } });
  flushSync();
}
function button(name: string) {
  const element = [...document.querySelectorAll('button')].find(element => element.textContent?.trim() === name || element.getAttribute('aria-label') === name);
  if (!element) throw new Error(`Missing button ${name}`);
  return element;
}
function select(name: string) {
  button(name).click();
  flushSync();
}
function input(label: string, value: string) {
  const element = [...document.querySelectorAll('input')].find(element => element.getAttribute('aria-label') === label || element.closest('label')?.textContent?.includes(label));
  if (!element) throw new Error(`Missing input ${label}`);
  element.value = value;
  element.dispatchEvent(new Event('input', { bubbles: true }));
  flushSync();
  return element;
}

it('starts on General and navigates tabs with wrapping focus and reset scrolling', async () => {
  await open();
  expect([...document.querySelectorAll('[role="tab"]')].map(tab => tab.textContent)).toEqual(['General', 'Clicks', 'Keys']);
  expect(button('General').getAttribute('aria-selected')).toBe('true');
  expect(button('Clicks').tabIndex).toBe(-1);
  const panel = document.querySelector<HTMLElement>('[role="tabpanel"]')!;
  panel.scrollTop = 180;
  button('General').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true }));
  flushSync();
  expect(document.activeElement).toBe(button('Keys'));
  expect(button('Keys').getAttribute('aria-selected')).toBe('true');
  expect(panel.getAttribute('aria-labelledby')).toBe(button('Keys').id);
  expect(panel.scrollTop).toBe(0);
  button('Keys').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
  flushSync();
  expect(document.activeElement).toBe(button('General'));
  panel.scrollTop = 90;
  select('Clicks');
  expect(panel.scrollTop).toBe(0);
  expect(button('Clicks').tabIndex).toBe(0);
  expect(document.body.textContent).toContain('Pointer halo');
  expect(document.body.textContent).not.toContain('Pill position');
  button('Clicks').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
  flushSync();
  expect(document.activeElement).toBe(button('Keys'));
});

it('saves key mode and pill sizes with the current session preferences', async () => {
  await open();
  select('Keys');
  expect(document.body.textContent).toContain('Typed text, including passwords, is never shown.');
  expect(document.body.textContent).toContain('Apps running as administrator can’t be seen.');
  button('All keys').click();
  expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, keys: 'all' });
  for (const [label, size] of [['Small', 'small'], ['Medium', 'medium'], ['Large', 'large']]) {
    button(label).click();
    expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, pillSize: size });
  }
  const preferences: Session['preferences'] = { ...DEFAULT_PREFERENCES, keys: 'all', rippleSize: 80 };
  await open({ preferences });
  select('Keys');
  expect(document.body.textContent).toContain('Everything you type appears on screen, including passwords.');
  button('Shortcuts only').click();
  expect(savePreferences).toHaveBeenLastCalledWith({ ...preferences, keys: 'shortcuts' });
});

it('saves every labeled position and the existing inactivity fade range on General', async () => {
  await open();
  for (const [label, position] of [['Top left', 'top-left'], ['Top right', 'top-right'], ['Bottom left', 'bottom-left'], ['Bottom center', 'bottom-center'], ['Bottom right', 'bottom-right']]) {
    button(label).click();
    expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, pillPosition: position });
  }
  expect(button('Bottom center').getAttribute('aria-pressed')).toBe('true');
  const fade = input('Fade after', '2300');
  expect([fade.min, fade.max, fade.step]).toEqual(['500', '5000', '100']);
  expect(document.body.textContent).toContain('s without a key');
  expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, fadeMs: 2300 });
});

it('saves click colors independently, ripple size and pointer halo', async () => {
  await open();
  select('Clicks');
  for (const [label, id] of [['Left', 'left'], ['Right', 'right'], ['Middle', 'middle']]) {
    input(`${label} button color`, '#123456');
    expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, rippleColors: { ...DEFAULT_PREFERENCES.rippleColors, [id]: '#123456' } });
  }
  const ripple = input('Ripple size', '100');
  expect([ripple.min, ripple.max, ripple.step]).toEqual(['24', '160', '4']);
  expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, rippleSize: 100 });
  document.querySelector<HTMLButtonElement>('[aria-labelledby="halo-label"]')!.click();
  expect(savePreferences).toHaveBeenLastCalledWith({ ...DEFAULT_PREFERENCES, halo: true });
  expect(action).not.toHaveBeenCalled();
});

it('preserves every permission branch and action', async () => {
  await open({ keyAccess: 'unknown' });
  expect(document.body.textContent).toContain('Clicks already work without it.');
  button('Allow…').click();
  expect(action).toHaveBeenLastCalledWith('request-key-access');
  await open({ keyAccess: 'denied' });
  expect(document.body.textContent).toContain('Clicks still show.');
  button('Open System Settings').click();
  expect(action).toHaveBeenLastCalledWith('request-key-access');
  await open({ keyAccess: 'granted' });
  expect(document.querySelector('#access-title')).toBeNull();
  await open({ keyAccess: 'granted', grantedWhileRunning: true });
  button('Relaunch').click();
  expect(action).toHaveBeenLastCalledWith('relaunch');
});

it('keeps platform privacy notes and shortcut conflict advice', async () => {
  vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue('Mac');
  await open({ shortcutUnavailable: true, shortcut: '⌘⇧K' });
  expect(document.body.textContent).toContain('Use the menu bar icon instead.');
  select('Keys');
  expect(document.body.textContent).toContain('Only keys pressed with ⌘, ⌃, or ⌥ appear.');
  await open({ preferences: { ...DEFAULT_PREFERENCES, keys: 'all' } });
  select('Keys');
  expect(document.body.textContent).toContain('Password fields stay hidden.');
  expect(document.body.textContent).not.toContain('administrator');
  vi.restoreAllMocks();
  await open({ shortcutUnavailable: true, shortcut: 'Ctrl+Alt+Shift+K' });
  expect(document.body.textContent).toContain('Use the tray icon instead.');
});

it('toggles through the shared action and closes with Close or Escape, and quits from any tab', async () => {
  await open({ enabled: true, shortcut: 'Ctrl+Alt+Shift+K' });
  expect(document.body.textContent).toContain('Toggle anytime with Ctrl+Alt+Shift+K');
  document.querySelector<HTMLButtonElement>('[aria-labelledby="enabled-label"]')!.click();
  expect(action).toHaveBeenCalledWith('toggle');
  button('Close settings').click();
  expect(action).toHaveBeenLastCalledWith('close-settings');
  select('Keys');
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true }));
  expect(action).toHaveBeenLastCalledWith('close-settings');
  button('Quit Glassview').click();
  expect(action).toHaveBeenLastCalledWith('quit');
});

it('keeps local action and save errors visible across tabs and dismisses them', async () => {
  await open();
  vi.mocked(action).mockRejectedValueOnce(new Error('Action failed'));
  button('Close settings').click();
  await Promise.resolve();
  flushSync();
  select('Clicks');
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('Action failed');
  button('Dismiss error').click();
  flushSync();
  expect(document.querySelector('[role="alert"]')).toBeNull();
  vi.mocked(savePreferences).mockRejectedValueOnce(new Error('Save failed'));
  input('Ripple size', '100');
  await Promise.resolve();
  flushSync();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('Save failed');
  button('Dismiss error').click();
  flushSync();
  expect(document.querySelector('[role="alert"]')).toBeNull();
});

it('dismisses session errors through the native action', async () => {
  await open({ error: 'Disk failed' });
  select('Keys');
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('Disk failed');
  button('Dismiss error').click();
  expect(action).toHaveBeenLastCalledWith('dismiss-error');
});
