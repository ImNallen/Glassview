import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import defaults from '../contract/preference-defaults.json';

export type Button = 'left' | 'right' | 'middle';
export type KeyMode = 'shortcuts' | 'all';
export type PillPosition = 'bottom-center' | 'bottom-left' | 'bottom-right' | 'top-left' | 'top-right';
export type PillSize = 'small' | 'medium' | 'large';
export interface Preferences {
  rippleColors: Record<Button, string>; rippleSize: number; halo: boolean;
  pillPosition: PillPosition; pillSize: PillSize; fadeMs: number; keys: KeyMode; onboarded: boolean;
}
export type KeyAccess = 'granted' | 'denied' | 'unknown';
export interface Session {
  enabled: boolean; preferences: Preferences; keyAccess: KeyAccess; grantedWhileRunning: boolean; settingsOpen: boolean;
  shortcut: string; shortcutUnavailable: boolean; error: string | null;
}
export type Action = 'toggle' | 'enable' | 'disable' | 'open-settings' | 'close-settings'
  | 'request-key-access' | 'relaunch' | 'dismiss-error' | 'quit';
export type StrokeView = { kind: 'chord'; label: string } | { kind: 'text'; text: string };
export interface Point { x: number; y: number }
export type OverlayEvent =
  | { kind: 'click'; button: Button; x: number; y: number }
  | { kind: 'halo'; at: Point | null }
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
/** Rust sends each overlay only its own display's events, so listen on this window rather than globally. */
export const onOverlay = (fn: (event: OverlayEvent) => void): Promise<UnlistenFn> =>
  getCurrentWebviewWindow().listen<OverlayEvent>('overlay', event => fn(event.payload));
