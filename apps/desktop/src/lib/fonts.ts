import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import '@fontsource-variable/nunito';
import type { PillFont } from './native';

const SYSTEM = "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif";
const SYSTEM_MONO = 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace';
const GEIST_MONO = `'Geist Mono Variable', ${SYSTEM_MONO}`;

export const PILL_FONTS: Record<PillFont, { label: string; family: string; mono: string }> = {
  geist: { label: 'Geist', family: `'Geist Variable', ${SYSTEM}`, mono: GEIST_MONO },
  system: { label: 'System', family: SYSTEM, mono: SYSTEM_MONO },
  rounded: { label: 'Rounded', family: `'Nunito Variable', ui-rounded, ${SYSTEM}`, mono: GEIST_MONO },
};
