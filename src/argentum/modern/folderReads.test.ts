// The column reads a disk or a folder when it is opened, and only then.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invoke, start } = vi.hoisted(() => ({
  invoke: vi.fn(),
  start: {
    drives: [{ path: 'C:\\' }, { path: 'D:\\' }, { path: 'E:\\' }],
    recent: [],
    startFolder: null,
    expanded: ['D:\\', 'E:\\'],
  },
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock('../ag', () => ({ ag: () => Promise.resolve(start) }));

import { useLibraryStore } from '../../store/useLibraryStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { rowWantsCount } from './folderCounts';
import { insertChildrenIntoTree, readFoldedDisks, startModernLibrary } from './folders';
import type { FolderTree } from '../../components/panel/right/FolderTree';

const tree = (path: string, children: FolderTree[] = []): FolderTree => ({
  name: path,
  path,
  children,
  isDir: true,
  hasSubdirs: children.length > 0,
});

const readDisks = () => invoke.mock.calls.flatMap(([, args]) => args.paths);
const rowOf = (path: string) => useLibraryStore.getState().folderTrees.find((t: { path: string }) => t.path === path);

describe('the disks at start', () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation((_: string, { paths }: { paths: string[] }) =>
      Promise.resolve(paths.map((p) => tree(p))),
    );
    useLibraryStore.setState({ folderTrees: [], rootPaths: [], expandedFolders: new Set(), currentFolderPath: null });
    useSettingsStore.setState({ appSettings: { lastFolderState: null } as never });
  });

  it('reads only the ones left unfolded', async () => {
    startModernLibrary({ current: null }, vi.fn());
    await vi.waitFor(() => expect(useLibraryStore.getState().folderTrees).toHaveLength(3));
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
    expect(readDisks()).toEqual(['D:\\', 'E:\\']);
    expect(rowOf('C:\\')).toMatchObject({ unread: true, hasSubdirs: true, name: 'C:' });
  });

  it('shows the others while one never answers', async () => {
    invoke.mockImplementation((_: string, { paths }: { paths: string[] }) =>
      paths[0] === 'E:\\' ? new Promise(() => {}) : Promise.resolve([tree(paths[0], [tree('D:\\Photos')])]),
    );
    startModernLibrary({ current: null }, vi.fn());
    await vi.waitFor(() => expect(rowOf('D:\\')?.children).toHaveLength(1));
    expect(rowOf('E:\\')).toMatchObject({ unread: true });
  });
});

describe('a folder opened from outside the tree', () => {
  it('reads its disk for the folders it unfolds', async () => {
    invoke.mockReset();
    invoke.mockImplementation((_: string, { paths }: { paths: string[] }) =>
      Promise.resolve(paths.map((p) => tree(p))),
    );
    useLibraryStore.setState({ folderTrees: [], rootPaths: [], expandedFolders: new Set(), currentFolderPath: null });
    useSettingsStore.setState({ appSettings: { lastFolderState: null } as never });
    startModernLibrary({ current: null }, vi.fn());
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));

    invoke.mockClear();
    const { expandedFolders } = useLibraryStore.getState();
    useLibraryStore.setState({
      currentFolderPath: 'C:\\2026\\June',
      expandedFolders: new Set([...expandedFolders, 'C:\\', 'C:\\2026']),
    });
    await vi.waitFor(() => expect(readDisks()).toEqual(['C:\\']));
    expect(invoke.mock.calls[0][1].expandedFolders).toContain('C:\\2026');
  });
});

describe('a photo opened before the column fills', () => {
  it('has its disk read for the folders above it', async () => {
    invoke.mockReset();
    invoke.mockImplementation((_: string, { paths }: { paths: string[] }) =>
      Promise.resolve(paths.map((p) => tree(p))),
    );
    useLibraryStore.setState({
      folderTrees: [],
      rootPaths: [],
      expandedFolders: new Set(),
      currentFolderPath: 'C:\\2026\\June',
    });
    useSettingsStore.setState({ appSettings: { lastFolderState: null } as never });
    const selectFolder = vi.fn(async () => {
      const { expandedFolders } = useLibraryStore.getState();
      useLibraryStore.setState({ expandedFolders: new Set([...expandedFolders, 'C:\\', 'C:\\2026']) });
    });
    startModernLibrary({ current: null }, selectFolder);
    await vi.waitFor(() => expect(readDisks()).toContain('C:\\'));
    const read = invoke.mock.calls.find(([, args]) => args.paths[0] === 'C:\\');
    expect(read?.[1].expandedFolders).toContain('C:\\2026');
  });
});

describe('a search of the column', () => {
  it('reads the folded disks and leaves the rest', async () => {
    invoke.mockReset();
    invoke.mockImplementation((_: string, { paths }: { paths: string[] }) =>
      Promise.resolve(paths.map((p) => tree(p, [tree(`${p}Trips`)]))),
    );
    useLibraryStore.setState({
      folderTrees: [{ ...tree('C:\\'), unread: true }, tree('D:\\', [tree('D:\\Photos')])],
      expandedFolders: new Set(['D:\\']),
    });
    readFoldedDisks();
    await vi.waitFor(() => expect(rowOf('C:\\')?.children).toHaveLength(1));
    expect(readDisks()).toEqual(['C:\\']);
    expect(rowOf('C:\\')?.unread).toBeUndefined();
  });
});

describe('a folder opened in the tree', () => {
  it('settles the arrow it only guessed', () => {
    const guessed = { ...tree('C:\\Users'), hasSubdirs: true, unread: true };
    const root = tree('C:\\', [guessed]);
    const [row] = insertChildrenIntoTree(root, 'C:\\Users', []).children;
    expect(row).toMatchObject({ hasSubdirs: false, unread: false });
  });

  it('keeps the arrow when there are folders inside', () => {
    const opened = insertChildrenIntoTree(tree('C:\\'), 'C:\\', [tree('C:\\Users')]);
    expect(opened.hasSubdirs).toBe(true);
  });
});

describe('a row and its count', () => {
  it('asks for it where looking is free', () => {
    expect(rowWantsCount({ open: false, selected: false })).toBe(true);
  });

  it('waits for a folder nothing may read to be opened or shown', () => {
    expect(rowWantsCount({ unread: true, open: false, selected: false })).toBe(false);
    expect(rowWantsCount({ unread: true, open: true, selected: false })).toBe(true);
    expect(rowWantsCount({ unread: true, open: false, selected: true })).toBe(true);
  });
});
