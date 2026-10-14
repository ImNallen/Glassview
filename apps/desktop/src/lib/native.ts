import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import defaults from '../contract/preference-defaults.json';

export type Button = 'left' | 'right' | 'middle' | 'back' | 'forward';
export type KeyMode = 'shortcuts' | 'all';
export type PillPosition = 'bottom-center' | 'bottom-left' | 'bottom-right' | 'top-left' | 'top-right';
export type PillSize = 'small' | 'medium' | 'large';
export type PillFont = 'geist' | 'system' | 'rounded';
export interface Preferences {
  rippleColors: Record<'left' | 'right' | 'middle', string>; rippleSize: number; halo: boolean;
  pillPosition: PillPosition; pillSize: PillSize; pillFont: PillFont; fadeMs: number; keys: KeyMode; onboarded: boolean;
}
export type KeyAccess = 'granted' | 'denied' | 'unknown';
export interface Session {
  enabled: boolean; preferences: Preferences; keyAccess: KeyAccess; grantedWhileRunning: boolean; settingsOpen: boolean;
  shortcut: string; shortcutUnavailable: boolean; error: string | null;
}
export type Action = 'toggle' | 'enable' | 'disable' | 'open-settings' | 'close-settings' | 'open-github'
  | 'request-key-access' | 'relaunch' | 'dismiss-error' | 'quit';
export type StrokeView = { kind: 'chord'; label: string } | { kind: 'text'; text: string };
export interface Point { x: number; y: number }
export type ScrollDirection = 'up' | 'down' | 'left' | 'right';
export type OverlayEvent =
  | { kind: 'click'; button: Button; mods: string | null; x: number; y: number }
  | { kind: 'halo'; at: Point | null }
  | { kind: 'scroll'; x: number; y: number; direction: ScrollDirection }
  | { kind: 'key'; stroke: StrokeView };

export const DEFAULT_PREFERENCES: Preferences = defaults as Preferences;
export const DEFAULT_SESSION: Session = {
  enabled: true, preferences: DEFAULT_PREFERENCES, keyAccess: 'granted', grantedWhileRunning: false, settingsOpen: false,
  shortcut: '', shortcutUnavailable: false, error: null,
};

export async function subscribe(fn: (session: Session) => void): Promise<UnlistenFn> {
  let received = false;
  const unlisten = await listen<Session>('session', event => { received = true; fn(event.payload); });
  try { const initial = await invoke<Session>('get_session'); if (!received) fn(initial); }
  catch (error) { unlisten(); throw error; }
  return unlisten;
}
export const action = (action: Action): Promise<void> => invoke('action', { action });
export const savePreferences = (preferences: Preferences): Promise<void> => invoke('set_preferences', { preferences });
/** Rust keeps a new Settings window hidden until this, so the page never shows default values. */
export const settingsReady = (): Promise<void> => invoke('settings_ready');
/** Rust emits each overlay's events under that overlay's own window label; see the pipeline thread for why. */
export const onOverlay = (fn: (event: OverlayEvent) => void): Promise<UnlistenFn> => {
  const window = getCurrentWebviewWindow();
  return window.listen<OverlayEvent>(window.label, event => fn(event.payload));
};
