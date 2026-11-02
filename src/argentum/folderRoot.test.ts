// Which root the folder tree expands to (rootContaining in hooks/useAppNavigation.ts).
// RapidRAW d6d90783.
import { describe, expect, it } from 'vitest';
import { rootContaining } from './folderRoot';

describe('the root a folder opens inside', () => {
  it('is not a root whose name the path only starts with', () => {
    expect(rootContaining('D:\\Photos2\\2026', ['D:\\Photos', 'D:\\Photos2'])).toBe('D:\\Photos2');
    expect(rootContaining('D:\\Photos2\\2026', ['D:\\Photos'])).toBeUndefined();
  });

  it('is the deepest root holding it, as when a pinned folder sits inside a root', () => {
    const roots = ['D:\\Photos', 'D:\\Photos\\2026'];
    expect(rootContaining('D:\\Photos\\2026\\October', roots)).toBe('D:\\Photos\\2026');
    expect(rootContaining('D:\\Photos\\2025', roots)).toBe('D:\\Photos');
  });

  it('is the root itself, and a drive root with its trailing separator', () => {
    expect(rootContaining('D:\\Photos', ['D:\\Photos'])).toBe('D:\\Photos');
    expect(rootContaining('D:\\Photos\\2026', ['D:\\'])).toBe('D:\\');
  });

  it('works with forward slashes', () => {
    expect(rootContaining('/Users/me/Photos2/a', ['/Users/me/Photos', '/Users/me/Photos2'])).toBe('/Users/me/Photos2');
    expect(rootContaining('/Volumes/SD/DCIM', ['/'])).toBe('/');
  });
});
