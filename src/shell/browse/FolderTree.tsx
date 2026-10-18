// The left of Browse: favourite folders, recent ones, then the computer's
// folders as a tree, each folder's folders read the first time it is opened.
//
// Keys as in Windows Explorer's folder pane: Up and Down move, Right opens a
// folder or goes to its first folder, Left closes it or goes to its parent,
// Home and End go to the ends, Enter shows the folder's photos. A folder is
// pinned to the favourites with its star, or from its menu (right click, or
// the menu key).

import { ChevronDown, ChevronRight, Clock, Folder as FolderIcon, HardDrive, Star } from 'lucide-react';
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';

import { text } from '../i18n';
import { folderName, samePath } from '../paths';
import { useShell, useShellStore } from '../ShellContext';
import type { Folder } from './browse.generated';

/** Where a menu is open, and for which folder. */
interface Menu {
  path: string;
  x: number;
  y: number;
}

type OpenMenu = (menu: Menu) => void;

const useIsFavourite = (path: string) => useShell((s) => s.places?.favourites.some((f) => samePath(f, path)) ?? false);

/**
 * The menu for `path`: where the pointer is, or under the row when the
 * keyboard asked for it (the event then has no position).
 */
function menuAt(path: string, e: { clientX: number; clientY: number; currentTarget: Element }): Menu {
  if (e.clientX !== 0 || e.clientY !== 0) return { path, x: e.clientX, y: e.clientY };
  const box = e.currentTarget.getBoundingClientRect();
  return { path, x: box.left + 24, y: box.bottom };
}

/** The star that pins a folder, on its row; always shown once pinned. */
function PinStar({ path }: { path: string }) {
  const store = useShellStore();
  const pinned = useIsFavourite(path);
  return (
    <button
      type="button"
      tabIndex={-1}
      className={pinned ? 'agb-row-pin is-pinned' : 'agb-row-pin'}
      title={text(pinned ? 'browse.unpin' : 'browse.pin')}
      aria-pressed={pinned}
      onClick={() => void store.getState().setFavourite(path, !pinned)}
    >
      <Star size={13} />
    </button>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="agb-group" role="group" aria-label={title}>
      <h3 className="agb-group-title">{title}</h3>
      {children}
    </section>
  );
}

/** A remembered folder: one click shows it. */
function Remembered({ path, icon, onMenu }: { path: string; icon: ReactNode; onMenu: OpenMenu }) {
  const store = useShellStore();
  const current = useShell((s) => s.folder !== null && samePath(s.folder, path));
  return (
    <div className={current ? 'agb-row is-current' : 'agb-row'}>
      <button
        type="button"
        className="agb-row-main"
        data-row=""
        data-path={path}
        title={path}
        onClick={() => void store.getState().browseFolder(path)}
        onContextMenu={(e) => {
          e.preventDefault();
          onMenu(menuAt(path, e));
        }}
      >
        {icon}
        <span className="agb-row-name">{folderName(path)}</span>
      </button>
      <PinStar path={path} />
    </div>
  );
}

function Node({ folder, depth, drive, onMenu }: { folder: Folder; depth: number; drive?: boolean; onMenu: OpenMenu }) {
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
          tabIndex={-1}
          className="agb-twisty"
          aria-label={folder.name}
          disabled={!folder.hasFolders}
          onClick={() => void store.getState().toggleExpanded(folder.path)}
        >
          {folder.hasFolders && (open ? <ChevronDown size={13} /> : <ChevronRight size={13} />)}
        </button>
        <button
          type="button"
          className="agb-row-main"
          role="treeitem"
          aria-expanded={folder.hasFolders ? open : undefined}
          aria-level={depth + 1}
          data-row=""
          data-tree=""
          data-path={folder.path}
          data-depth={depth}
          data-expandable={folder.hasFolders ? '' : undefined}
          data-open={open ? '' : undefined}
          title={folder.path}
          onClick={() => void store.getState().browseFolder(folder.path)}
          onDoubleClick={() => void store.getState().toggleExpanded(folder.path)}
          onContextMenu={(e) => {
            e.preventDefault();
            onMenu(menuAt(folder.path, e));
          }}
        >
          <Icon size={14} />
          <span className="agb-row-name">{folder.name}</span>
        </button>
        <PinStar path={folder.path} />
      </div>
      {open && inside?.map((f) => <Node key={f.path} folder={f} depth={depth + 1} onMenu={onMenu} />)}
    </>
  );
}

/** A folder's menu: show its photos, pin or unpin it. Escape or a click elsewhere closes it. */
function FolderMenu({ menu, onClose }: { menu: Menu; onClose: () => void }) {
  const store = useShellStore();
  const pinned = useIsFavourite(menu.path);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    box.current?.querySelector<HTMLElement>('button')?.focus();
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener('mousedown', away);
    return () => window.removeEventListener('mousedown', away);
  }, [onClose]);
  const choose = (act: () => void) => () => {
    act();
    onClose();
  };
  const keys = (e: KeyboardEvent) => {
    const items = [...(box.current?.querySelectorAll<HTMLElement>('button') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') onClose();
    else if (e.key === 'ArrowDown') items[(at + 1) % items.length]?.focus();
    else if (e.key === 'ArrowUp') items[(at - 1 + items.length) % items.length]?.focus();
    else return;
    e.preventDefault();
    e.stopPropagation();
  };
  return (
    <div ref={box} className="agb-menu" role="menu" style={{ left: menu.x, top: menu.y }} onKeyDown={keys}>
      <button type="button" role="menuitem" onClick={choose(() => void store.getState().browseFolder(menu.path))}>
        {text('browse.showPhotos')}
      </button>
      <button
        type="button"
        role="menuitem"
        onClick={choose(() => void store.getState().setFavourite(menu.path, !pinned))}
      >
        <Star size={13} />
        {text(pinned ? 'browse.unpin' : 'browse.pin')}
      </button>
    </div>
  );
}

/** The rows the keys move between, in the order shown. */
const rowsIn = (nav: HTMLElement) => [...nav.querySelectorAll<HTMLElement>('[data-row]')];

export function FolderTree() {
  const store = useShellStore();
  const places = useShell((s) => s.places);
  const [menu, setMenu] = useState<Menu | null>(null);
  const nav = useRef<HTMLElement>(null);
  const backTo = useRef<HTMLElement | null>(null);

  const openMenu = (m: Menu) => {
    backTo.current = document.activeElement as HTMLElement | null;
    setMenu(m);
  };
  const closeMenu = () => {
    setMenu(null);
    backTo.current?.focus();
  };

  const keys = (e: KeyboardEvent<HTMLElement>) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>('[data-row]');
    if (!row || !nav.current) return;
    const rows = rowsIn(nav.current);
    const at = rows.indexOf(row);
    const path = row.dataset.path!;
    const depth = Number(row.dataset.depth);
    const go = (to: HTMLElement | undefined) => to?.focus();
    switch (e.key) {
      case 'ArrowDown':
        go(rows[at + 1]);
        break;
      case 'ArrowUp':
        go(rows[at - 1]);
        break;
      case 'Home':
        go(rows[0]);
        break;
      case 'End':
        go(rows[rows.length - 1]);
        break;
      case 'ArrowRight':
        if (row.dataset.tree === undefined || row.dataset.expandable === undefined) return;
        if (row.dataset.open === undefined) void store.getState().toggleExpanded(path);
        else if (Number(rows[at + 1]?.dataset.depth) === depth + 1) go(rows[at + 1]);
        break;
      case 'ArrowLeft':
        if (row.dataset.tree === undefined) return;
        if (row.dataset.open !== undefined) void store.getState().toggleExpanded(path);
        else
          go(
            rows
              .slice(0, at)
              .reverse()
              .find((r) => Number(r.dataset.depth) === depth - 1),
          );
        break;
      case 'Enter':
        void store.getState().browseFolder(path);
        break;
      // The menu key and Shift+F10 arrive as the row's contextmenu event.
      default:
        return;
    }
    e.preventDefault();
  };

  if (!places) return <nav className="agb-tree" />;
  return (
    <nav ref={nav} className="agb-tree" role="tree" aria-label={text('browse.folders')} onKeyDown={keys}>
      <Group title={text('browse.favourites')}>
        {places.favourites.map((p) => (
          <Remembered key={p} path={p} icon={<Star size={14} />} onMenu={openMenu} />
        ))}
        {places.favourites.length === 0 && <p className="agb-group-hint">{text('browse.favouritesHint')}</p>}
      </Group>
      {places.recent.length > 0 && (
        <Group title={text('browse.recent')}>
          {places.recent.map((p) => (
            <Remembered key={p} path={p} icon={<Clock size={14} />} onMenu={openMenu} />
          ))}
        </Group>
      )}
      <Group title={text('browse.folders')}>
        {places.standard.map((f) => (
          <Node key={f.path} folder={f} depth={0} onMenu={openMenu} />
        ))}
        {places.drives.map((f) => (
          <Node key={f.path} folder={f} depth={0} drive onMenu={openMenu} />
        ))}
      </Group>
      {menu && <FolderMenu menu={menu} onClose={closeMenu} />}
    </nav>
  );
}
