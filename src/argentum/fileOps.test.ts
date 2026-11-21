import { describe, expect, it } from 'vitest';
import { activePathAfterDelete, isInFolder, isShownWithin, pathAfterFolderRename, shownAfterRename } from './fileOps';

describe('the photo made active after a delete', () => {
  const order = ['/p/a.CR2', '/p/b.CR2', '/p/b.CR2?vc=a1b2c3', '/p/b.CR2?vc=b2c3d4', '/p/c.CR2'];

  it('skips the virtual copies of the deleted photo, which go with it', () => {
    expect(activePathAfterDelete(order, '/p/b.CR2', ['/p/b.CR2'])).toBe('/p/c.CR2');
  });

  it('goes back when nothing after it stays', () => {
    expect(activePathAfterDelete(order, '/p/b.CR2', ['/p/b.CR2', '/p/c.CR2'])).toBe('/p/a.CR2');
  });

  it('counts a virtual copy as deleted when its photo is', () => {
    expect(activePathAfterDelete(order, '/p/b.CR2?vc=a1b2c3', ['/p/b.CR2'])).toBe('/p/c.CR2');
  });

  it('keeps the other copies when only one copy is deleted', () => {
    expect(activePathAfterDelete(order, '/p/b.CR2?vc=a1b2c3', ['/p/b.CR2?vc=a1b2c3'])).toBe('/p/b.CR2?vc=b2c3d4');
  });

  it('keeps the active photo when it is not deleted, and finds none when everything goes', () => {
    expect(activePathAfterDelete(order, '/p/a.CR2', ['/p/b.CR2'])).toBe('/p/a.CR2');
    expect(activePathAfterDelete(['/p/b.CR2', '/p/b.CR2?vc=a1b2c3'], '/p/b.CR2', ['/p/b.CR2'])).toBeNull();
  });
});

describe('paths after a folder rename', () => {
  it('moves the folder and what is inside it, on either separator', () => {
    expect(pathAfterFolderRename('/photos/trip', '/photos/trip', '/photos/rome')).toBe('/photos/rome');
    expect(pathAfterFolderRename('/photos/trip/day 1', '/photos/trip', '/photos/rome')).toBe('/photos/rome/day 1');
    expect(pathAfterFolderRename('C:\\photos\\trip\\day 1', 'C:\\photos\\trip', 'C:\\photos\\rome')).toBe(
      'C:\\photos\\rome\\day 1',
    );
  });

  it('leaves a sibling folder that starts with the same letters alone', () => {
    expect(isInFolder('/photos/trip2', '/photos/trip')).toBe(false);
    expect(pathAfterFolderRename('/photos/trip2', '/photos/trip', '/photos/rome')).toBe('/photos/trip2');
    expect(pathAfterFolderRename('/photos/trip2/day 1', '/photos/trip', '/photos/rome')).toBe('/photos/trip2/day 1');
  });
});

describe('the photo on show after it is renamed', () => {
  it('opens the editor only when it was open', () => {
    expect(shownAfterRename('/p/trip_1.CR2', false)).toEqual({ select: '/p/trip_1.CR2', openInEditor: false });
    expect(shownAfterRename('/p/trip_1.CR2', true)).toEqual({ select: '/p/trip_1.CR2', openInEditor: true });
  });

  it('leaves the editor without a new path, and the grid alone', () => {
    expect(shownAfterRename(undefined, true)).toBe('backToLibrary');
    expect(shownAfterRename(undefined, false)).toBe('nothing');
  });
});

describe('the folders whose photos can be on show', () => {
  it('counts a folder inside the one on show, on either separator', () => {
    expect(isShownWithin('/photos/trip', '/photos')).toBe(true);
    expect(isShownWithin('C:\\photos\\trip\\day 1', 'C:\\photos')).toBe(true);
  });

  it('leaves out the folder itself, a sibling with the same letters, and no folder at all', () => {
    expect(isShownWithin('/photos', '/photos')).toBe(false);
    expect(isShownWithin('/photos2/trip', '/photos')).toBe(false);
    expect(isShownWithin('/photos/trip', null)).toBe(false);
  });
});
