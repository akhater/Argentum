// Settings' clear buttons work on the folder open in the library, never on the disks in the folder column.

import { renderToStaticMarkup } from 'react-dom/server';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { useLibraryStore } from '../../store/useLibraryStore';
import SettingsPanel from '../../components/panel/SettingsPanel';
import en from '../../i18n/locales/en.json';
import { folderOnDisk } from './folders';

vi.mock('@tauri-apps/api/core', () => ({ invoke: () => Promise.resolve() }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }));
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: () => Promise.resolve() }));
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'windows' }));

const noop = () => {};
const DISKS = ['C:\\', 'D:\\'];

/** The library as Settings finds it; a server render reads the store's first state. */
function setLibrary(currentFolderPath: string | null) {
  const state = { rootPaths: DISKS, currentFolderPath };
  Object.assign(useLibraryStore.getInitialState(), state);
  useLibraryStore.setState(state);
}

/** A Settings section, as HTML. */
function page(section?: string) {
  return renderToStaticMarkup(
    <SettingsPanel
      appSettings={{}}
      onLibraryRefresh={noop}
      onSettingsChange={() => Promise.resolve()}
      modernCategory={section}
    />,
  );
}

/** The button under the heading `title`, and whether it can be pressed. */
function button(html: string, title: string) {
  const after = html.slice(html.indexOf(`>${title}<`));
  const tag = after.match(/<button[^>]*>/)?.[0] ?? '';
  return { found: tag !== '', enabled: tag !== '' && !/\sdisabled=""/.test(tag) };
}

beforeAll(async () => {
  await i18n.use(initReactI18next).init({ lng: 'en', resources: { en: { translation: en } } });
});

describe('the folder a clear works on', () => {
  it('is the open folder', () => {
    expect(folderOnDisk('D:\\Photos\\Trip')).toBe('D:\\Photos\\Trip');
  });

  it('is none with no folder or an album open', () => {
    expect(folderOnDisk(null)).toBeNull();
    expect(folderOnDisk('Album: Best of')).toBeNull();
  });
});

describe('Clear sidecars', () => {
  const title = en.settings.data.clearSidecars;

  it('names the open folder, not the disks', () => {
    setLibrary('D:\\Photos\\Trip');
    const html = page('processing');
    expect(html).toContain('D:\\Photos\\Trip');
    expect(html).not.toContain('C:\\');
    expect(html).toContain('Recycle Bin');
    expect(button(html, title)).toEqual({ found: true, enabled: true });
  });

  it('cannot be pressed with no folder open', () => {
    setLibrary(null);
    expect(button(page('processing'), title)).toEqual({ found: true, enabled: false });
  });

  it('cannot be pressed with an album open', () => {
    setLibrary('Album: Best of');
    expect(button(page('processing'), title)).toEqual({ found: true, enabled: false });
  });
});

// Only the page without a Modern section has them, and the Modern window always gives one.
describe('Clear tags', () => {
  for (const title of [en.settings.tagging.clearAllTagsTitle, en.settings.tagging.clearAiTagsTitle]) {
    it(`${title} can be pressed with a folder open, not without`, () => {
      setLibrary('D:\\Photos\\Trip');
      expect(button(page(), title)).toEqual({ found: true, enabled: true });
      setLibrary(null);
      expect(button(page(), title)).toEqual({ found: true, enabled: false });
    });
  }
});
