const SURFACES = ['overlay', 'settings'] as const;
export type Surface = (typeof SURFACES)[number];

export function parseSurface(search: string): Surface | null {
  const value = new URLSearchParams(search).get('surface');
  return SURFACES.find(surface => surface === value) ?? null;
}
