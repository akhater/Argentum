// The left of Browse: favourite folders, recent ones, then the computer's
// folders as a tree, each folder's folders read the first time it is opened.

import { ChevronDown, ChevronRight, Clock, Folder as FolderIcon, HardDrive, Star } from 'lucide-react';
import type { ReactNode } from 'react';

import { text } from '../i18n';
import { folderName, samePath } from '../paths';
import { useShell, useShellStore } from '../ShellContext';
import type { Folder } from './browse.generated';

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="agb-group">
      <h3 className="agb-group-title">{title}</h3>
      {children}
    </section>
  );
}

/** A remembered folder: one click shows it. */
function Remembered({ path, icon }: { path: string; icon: ReactNode }) {
  const store = useShellStore();
  const current = useShell((s) => s.folder !== null && samePath(s.folder, path));
  return (
    <button
      type="button"
      className={current ? 'agb-row is-current' : 'agb-row'}
      title={path}
      onClick={() => void store.getState().browseFolder(path)}
    >
      {icon}
      <span className="agb-row-name">{folderName(path)}</span>
    </button>
  );
}

function Node({ folder, depth, drive }: { folder: Folder; depth: number; drive?: boolean }) {
  const store = useShellStore();
  const open = useShell((s) => s.expanded[folder.path] ?? false);
  const inside = useShell((s) => s.subfolders[folder.path]);
  const current = useShell((s) => s.folder !== null && samePath(s.folder, folder.path));
  const Icon = drive ? HardDrive : FolderIcon;
  return (
    <>
      <div className={current ? 'agb-row is-current' : 'agb-row'} style={{ paddingLeft: 6 + depth * 14 }}>
        <button
          type="button"
          className="agb-twisty"
          aria-label={folder.name}
          aria-expanded={open}
          disabled={!folder.hasFolders}
          onClick={() => void store.getState().toggleExpanded(folder.path)}
        >
          {folder.hasFolders && (open ? <ChevronDown size={13} /> : <ChevronRight size={13} />)}
        </button>
        <button
          type="button"
          className="agb-row-main"
          title={folder.path}
          onClick={() => void store.getState().browseFolder(folder.path)}
          onDoubleClick={() => void store.getState().toggleExpanded(folder.path)}
        >
          <Icon size={14} />
          <span className="agb-row-name">{folder.name}</span>
        </button>
      </div>
      {open && inside?.map((f) => <Node key={f.path} folder={f} depth={depth + 1} />)}
    </>
  );
}

export function FolderTree() {
  const places = useShell((s) => s.places);
  if (!places) return <nav className="agb-tree" />;
  return (
    <nav className="agb-tree">
      {places.favourites.length > 0 && (
        <Group title={text('browse.favourites')}>
          {places.favourites.map((p) => (
            <Remembered key={p} path={p} icon={<Star size={14} />} />
          ))}
        </Group>
      )}
      {places.recent.length > 0 && (
        <Group title={text('browse.recent')}>
          {places.recent.map((p) => (
            <Remembered key={p} path={p} icon={<Clock size={14} />} />
          ))}
        </Group>
      )}
      <Group title={text('browse.folders')}>
        {places.standard.map((f) => (
          <Node key={f.path} folder={f} depth={0} />
        ))}
        {places.drives.map((f) => (
          <Node key={f.path} folder={f} depth={0} drive />
        ))}
      </Group>
    </nav>
  );
}
