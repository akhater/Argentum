// An active library filter can be seen: on the View button, and in an emptied grid.

import type { ReactElement, ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { useLibraryStore } from '../../store/useLibraryStore';
import {
  EditedStatus,
  FilterCriteria,
  LibraryViewMode,
  RawStatus,
  ThumbnailAspectRatio,
  ThumbnailSize,
} from '../../components/ui/AppProperties';
import MainLibrary from '../../components/panel/MainLibrary';
import { AG_NS } from '../locales';
import en from '../locales/en.json';
import NoPhotosMatch from './NoPhotosMatch';
import { shownFiltersOnly } from './shownFilters';
import TopBar from './TopBar';

// The window itself is Tauri's, and these tests render without one.
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }));

const noop = () => {};

const topBar = () =>
  renderToStaticMarkup(
    <TopBar
      onBack={noop}
      onForward={noop}
      onOpenPath={noop}
      onZoom={noop}
      thumbnailSize={ThumbnailSize.Medium}
      setThumbnailSize={noop}
      thumbnailAspectRatio={ThumbnailAspectRatio.Cover}
      setThumbnailAspectRatio={noop}
      libraryViewMode={LibraryViewMode.Flat}
      setLibraryViewMode={noop}
    />,
  );

const emptyGrid = () =>
  renderToStaticMarkup(
    <MainLibrary
      activePath={null}
      aiModelDownloadStatus={null}
      appSettings={null}
      currentFolderPath="D:\Photos"
      groupBadgeInfo={null}
      imageList={[]}
      imageRatings={{}}
      indexingProgress={{ current: 0, total: 0 }}
      isIndexing={false}
      libraryViewMode={LibraryViewMode.Flat}
      multiSelectedPaths={[]}
      onClearSelection={noop}
      onContextMenu={noop}
      onEmptyAreaContextMenu={noop}
      onImageClick={noop}
      onImageDoubleClick={noop}
      onThumbnailSizeChange={noop}
      rootPaths={['D:\\']}
      thumbnailAspectRatio={ThumbnailAspectRatio.Cover}
      thumbnailSize={ThumbnailSize.Medium}
    />,
  );

/** Set the filters, also where a server render reads them: zustand gives it the store's first state. */
function setFilters(saved: Partial<FilterCriteria>) {
  const filterCriteria = shownFiltersOnly(saved);
  Object.assign(useLibraryStore.getInitialState(), { filterCriteria });
  useLibraryStore.setState({ filterCriteria });
}

/** Each button's text, with its icons left out. */
const buttonTexts = (html: string) =>
  [...html.matchAll(/<button[^>]*>(.*?)<\/button>/g)].map((m) => m[1].replace(/<[^>]*>/g, ''));

/** The element a component returned, with hooks running as they do in a render. */
function renderedTree(component: () => ReactNode) {
  let tree: ReactNode;
  const Probe = () => {
    tree = component();
    return <>{tree}</>;
  };
  renderToStaticMarkup(<Probe />);
  return tree;
}

function findButton(node: ReactNode): ReactElement<{ onClick(): void }> | undefined {
  if (Array.isArray(node)) return node.map(findButton).find(Boolean);
  if (typeof node !== 'object' || node === null || !('type' in node)) return undefined;
  const element = node as ReactElement<{ children?: ReactNode; onClick(): void }>;
  return element.type === 'button' ? element : findButton(element.props.children);
}

beforeAll(async () => {
  await i18n.use(initReactI18next).init({ lng: 'en', resources: { en: { [AG_NS]: en } } });
});

beforeEach(() => {
  setFilters({});
});

describe('the View button', () => {
  it('is plain View while every photo shows', () => {
    expect(buttonTexts(topBar())).toContain('View');
  });

  it('names the filter that is hiding photos', () => {
    setFilters({ rawStatus: RawStatus.RawOnly });
    expect(buttonTexts(topBar())).toContain('View: RAW only');
  });

  it('names both filters when both are on', () => {
    setFilters({ editedStatus: EditedStatus.UneditedOnly, rawStatus: RawStatus.NonRawOnly });
    expect(buttonTexts(topBar())).toContain('View: Not edited · JPEG, TIFF and others');
  });
});

describe('an emptied grid', () => {
  it('says which filter left it empty and offers to show all', () => {
    setFilters({ editedStatus: EditedStatus.EditedOnly });
    const html = emptyGrid();
    expect(html).toContain('No photos match: Edited only');
    expect(buttonTexts(html)).toEqual(['Show all']);
  });

  it('keeps its plain message when no filter is on', () => {
    const html = emptyGrid();
    expect(html).not.toContain('No photos match');
    expect(buttonTexts(html)).toEqual([]);
  });

  it('clears the filter on Show all', () => {
    setFilters({ editedStatus: EditedStatus.EditedOnly, rawStatus: RawStatus.RawOnly });
    const tree = renderedTree(() => NoPhotosMatch({ filters: 'Edited only · RAW only', onContextMenu: noop }));
    findButton(tree)?.props.onClick();
    const { editedStatus, rawStatus } = useLibraryStore.getState().filterCriteria;
    expect([editedStatus, rawStatus]).toEqual([EditedStatus.All, RawStatus.All]);
  });
});
