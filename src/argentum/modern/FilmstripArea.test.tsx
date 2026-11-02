// The thumbnail bar's arrows: on show with any photo to step to, whether or not one is selected.

import { renderToStaticMarkup } from 'react-dom/server';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { type ImageFile, ThumbnailAspectRatio } from '../../components/ui/AppProperties';
import { useLibraryStore } from '../../store/useLibraryStore';
import { AG_NS } from '../locales';
import en from '../locales/en.json';
import FilmstripArea from './FilmstripArea';

vi.mock('@tauri-apps/api/core', () => ({ invoke: () => Promise.resolve() }));
// The strip itself draws thumbnails through a virtual list; only the arrows either side are under test.
vi.mock('../../components/panel/Filmstrip', () => ({ default: () => null }));

const noop = () => {};
const photos = (...paths: string[]) => paths.map((path) => ({ path })) as ImageFile[];

/** The library as the bar finds it, also where a server render reads it: zustand gives it the store's first state. */
function setLibrary(currentFolderPath: string | null, libraryActivePath: string | null) {
  const state = { currentFolderPath, libraryActivePath };
  Object.assign(useLibraryStore.getInitialState(), state);
  useLibraryStore.setState(state);
}

/** Each arrow on show, by name, and whether it can be pressed. */
function arrows(imageList: ImageFile[]) {
  const html = renderToStaticMarkup(
    <FilmstripArea
      imageList={imageList}
      thumbnailAspectRatio={ThumbnailAspectRatio.Cover}
      onOpenInEditor={noop}
      onSelectInGrid={noop}
      onStepPhoto={noop}
      onClearSelection={noop}
      onContextMenu={noop}
      onEmptyAreaContextMenu={noop}
      onRequestThumbnails={noop}
      onResize={noop}
      onResetHeight={noop}
    />,
  );
  const buttons = [...html.matchAll(/<button[^>]*aria-label="([^"]*)"[^>]*>/g)];
  return Object.fromEntries(buttons.map(([tag, label]) => [label, !tag.includes('disabled')]));
}

beforeAll(async () => {
  await i18n.use(initReactI18next).init({ lng: 'en', resources: { en: { [AG_NS]: en } } });
});

describe("the thumbnail bar's arrows", () => {
  it('show with nothing selected, next on and previous greyed out', () => {
    setLibrary('D:\\Photos', null);
    expect(arrows(photos('D:\\Photos\\a.CR2', 'D:\\Photos\\b.CR2'))).toEqual({
      'Previous photo': false,
      'Next photo': true,
    });
  });

  it('show for a folder of one with nothing selected', () => {
    setLibrary('D:\\Photos', null);
    expect(arrows(photos('D:\\Photos\\a.CR2'))).toEqual({ 'Previous photo': false, 'Next photo': true });
  });

  it('stop at the ends with a photo selected', () => {
    setLibrary('D:\\Photos', 'D:\\Photos\\a.CR2');
    expect(arrows(photos('D:\\Photos\\a.CR2', 'D:\\Photos\\b.CR2'))).toEqual({
      'Previous photo': false,
      'Next photo': true,
    });
    setLibrary('D:\\Photos', 'D:\\Photos\\b.CR2');
    expect(arrows(photos('D:\\Photos\\a.CR2', 'D:\\Photos\\b.CR2'))).toEqual({
      'Previous photo': true,
      'Next photo': false,
    });
  });

  it('stay, greyed out, for a selected photo a filter left alone in an emptied list', () => {
    setLibrary('D:\\Photos', 'D:\\Photos\\a.CR2');
    expect(arrows([])).toEqual({ 'Previous photo': false, 'Next photo': false });
  });

  it('do not show for an empty folder', () => {
    setLibrary('D:\\Photos', null);
    expect(arrows([])).toEqual({});
  });

  it('do not show with no folder', () => {
    setLibrary(null, null);
    expect(arrows(photos('D:\\Photos\\a.CR2'))).toEqual({});
  });
});
