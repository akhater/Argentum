import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Snapshot } from './snapshotsStore';

const writes: unknown[] = [];
let read: () => Promise<Snapshot[]>;

vi.mock('../ag', () => ({
  ag: vi.fn(async (name: string, args: { snapshots?: Snapshot[] }) => {
    if (name === 'snapshots') return read();
    if (name === 'set_snapshots') writes.push(args.snapshots);
    return null;
  }),
}));

const { ag } = await import('../ag');
const { useSnapshots } = await import('./snapshotsStore');

const snapshot = (id: string): Snapshot => ({ id, name: id, at: 0, adjustments: {} as Snapshot['adjustments'] });

describe("a photo's snapshots", () => {
  beforeEach(() => {
    writes.length = 0;
    useSnapshots.setState({ byPath: {} });
  });

  it('are never written over when they could not be read', async () => {
    read = () => Promise.reject(new Error('sidecar locked'));
    await useSnapshots.getState().load('IMG.CR2');
    useSnapshots.getState().keep('IMG.CR2', [snapshot('new')]);
    expect(writes).toEqual([]);
  });

  it('are written once read', async () => {
    read = async () => [snapshot('old')];
    await useSnapshots.getState().load('IMG.CR2');
    useSnapshots.getState().keep('IMG.CR2', [snapshot('old'), snapshot('new')]);
    await vi.waitFor(() => expect(writes).toEqual([[snapshot('old'), snapshot('new')]]));
  });

  it('are written one save at a time, in the order asked', async () => {
    read = async () => [];
    await useSnapshots.getState().load('IMG.CR2');
    const events: string[] = [];
    let releaseFirst!: () => void;
    const firstDone = new Promise<void>((resolve) => (releaseFirst = resolve));
    let calls = 0;
    vi.mocked(ag).mockImplementation((async (_name: string, args: { snapshots?: Snapshot[] }) => {
      const id = args.snapshots?.[0]?.id ?? 'none';
      events.push(`start ${id}`);
      if (calls++ === 0) await firstDone;
      events.push(`end ${id}`);
      return null;
    }) as unknown as typeof ag);

    useSnapshots.getState().keep('IMG.CR2', [snapshot('first')]);
    useSnapshots.getState().keep('IMG.CR2', [snapshot('second')]);
    await vi.waitFor(() => expect(events).toEqual(['start first']));
    releaseFirst();
    await vi.waitFor(() => expect(events).toEqual(['start first', 'end first', 'start second', 'end second']));
  });
});
