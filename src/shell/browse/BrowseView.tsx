// Browse: folders on the left, the folder's photos in the middle. Before a
// folder is picked, the middle offers the favourite and recent ones; nothing
// else is remembered, so nothing else is shown.

import { Clock, Star } from 'lucide-react';

import { text } from '../i18n';
import { folderName, samePath } from '../paths';
import { useShell, useShellStore } from '../ShellContext';
import type { SortBy } from './browseSlice';
import { FolderTree } from './FolderTree';
import { PhotoGrid } from './PhotoGrid';
import './browse.css';

function FolderHead({ folder }: { folder: string }) {
  const store = useShellStore();
  const count = useShell((s) => s.folderPhotos.length);
  const listed = useShell((s) => s.listed);
  const sortBy = useShell((s) => s.sortBy);
  const pinned = useShell((s) => s.places?.favourites.some((f) => samePath(f, folder)) ?? false);
  return (
    <header className="agb-head">
      <span className="agb-head-name" title={folder}>
        {folderName(folder)}
      </span>
      <button
        type="button"
        className={pinned ? 'agb-pin is-pinned' : 'agb-pin'}
        title={text(pinned ? 'browse.unpin' : 'browse.pin')}
        aria-pressed={pinned}
        onClick={() => void store.getState().setFavourite(folder, !pinned)}
      >
        <Star size={14} />
      </button>
      <span className="ags-dim">
        {text('browse.count', { count })}
        {!listed && `  ·  ${text('browse.listing')}`}
      </span>
      <span className="ags-toolbar-spacer" />
      <label className="agb-sort">
        <span className="ags-dim">{text('browse.sortBy')}</span>
        <select value={sortBy} onChange={(e) => store.getState().setSortBy(e.target.value as SortBy)}>
          <option value="name">{text('browse.byName')}</option>
          <option value="date">{text('browse.byDate')}</option>
        </select>
      </label>
    </header>
  );
}

/** The middle before a folder is picked: the folders remembered. */
function Start() {
  const store = useShellStore();
  const places = useShell((s) => s.places);
  const folders = [
    ...(places?.favourites ?? []).map((path) => ({ path, Icon: Star })),
    ...(places?.recent ?? []).map((path) => ({ path, Icon: Clock })),
  ].filter((f, i, all) => all.findIndex((g) => samePath(g.path, f.path)) === i);
  return (
    <div className="agb-start">
      <p className="ags-dim">{text(folders.length > 0 ? 'browse.start' : 'browse.nothingYet')}</p>
      {folders.map(({ path, Icon }) => (
        <button
          key={path}
          type="button"
          className="agb-start-folder"
          onClick={() => void store.getState().browseFolder(path)}
        >
          <Icon size={16} />
          <span className="agb-start-name">{folderName(path)}</span>
          <span className="ags-dim">{path}</span>
        </button>
      ))}
    </div>
  );
}

export function BrowseView() {
  const folder = useShell((s) => s.folder);
  const error = useShell((s) => s.browseError);
  const empty = useShell((s) => s.listed && s.folderPhotos.length === 0);
  return (
    <div className="agb">
      <aside className="agb-side">
        <FolderTree />
      </aside>
      <section className="agb-middle">
        {folder === null ? (
          <Start />
        ) : (
          <>
            <FolderHead folder={folder} />
            {error && (
              <p className="agb-message ags-error" role="alert">
                {error.message}
              </p>
            )}
            {empty && !error && <p className="agb-message ags-dim">{text('browse.empty')}</p>}
            <PhotoGrid />
          </>
        )}
      </section>
    </div>
  );
}
