export const SUPER_RESOLUTION_OPEN_EVENT = 'argentum:open-super-resolution';

export function openSuperResolution(paths: string[]) {
  if (paths.length === 0) return;
  window.dispatchEvent(
    new CustomEvent<{ paths: string[] }>(SUPER_RESOLUTION_OPEN_EVENT, {
      detail: { paths },
    }),
  );
}
