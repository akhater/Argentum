import type { ReactElement, ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { initReactI18next } from 'react-i18next';
import i18n from 'i18next';
import type { ProviderLooks as Offer } from '../ag.generated';
import { AG_NS } from '../locales';
import en from '../locales/en.json';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('react-toastify', () => ({ toast: { success: vi.fn(), info: vi.fn(), error: vi.fn() } }));

const { default: ProviderLooks, LookStrip } = await import('./ProviderLooks');
const { useProviderLooks } = await import('./providerLooksStore');
const { useLibraryStore } = await import('../../store/useLibraryStore');
const { useUIStore } = await import('../../store/useUIStore');
const { useEditorStore } = await import('../../store/useEditorStore');

const OPEN = 'D:/photos/open.cr3';

const OFFER: Offer = {
  cameras: [
    { id: 'cam1', name: 'Camera One', make: 'Acme', model: 'One' },
    { id: 'cam2', name: 'Camera Two', make: 'Acme', model: 'Two' },
  ],
  looks: [
    { id: 'look1', name: 'Invented Stock', group: null },
    { id: 'look2', name: 'Other Stock', group: null },
  ],
  options: [],
  on: true,
  photo: { camera: 'cam1', files: [], previews: [], clearable: 0, ownWb: null },
};

/** A server render reads the store's first state. */
function offered(state: Partial<ReturnType<typeof useProviderLooks.getState>>) {
  Object.assign(useProviderLooks.getInitialState(), state);
  useProviderLooks.setState(state);
}

const section = () => renderToStaticMarkup(<ProviderLooks photo={OPEN} ready onTakeOff={() => {}} />);

const FIRST_SECTIONS = { ...useUIStore.getState().collapsibleSectionsState };

/** The store's sections, for a server render too. */
function syncFold() {
  Object.assign(useUIStore.getInitialState(), {
    collapsibleSectionsState: useUIStore.getState().collapsibleSectionsState,
  });
}

function folded(value: boolean) {
  useUIStore.setState((s) => ({
    collapsibleSectionsState: { ...s.collapsibleSectionsState, providerLooksFolded: value },
  }));
  syncFold();
}

beforeAll(async () => {
  await i18n.use(initReactI18next).init({ lng: 'en', resources: { en: { [AG_NS]: en } } });
});

beforeEach(() => {
  offered({
    offer: null,
    chosen: {},
    chosenOptions: {},
    waiting: {},
    note: null,
    batch: null,
    previewing: null,
    deleting: false,
  });
  useUIStore.setState({ collapsibleSectionsState: { ...FIRST_SECTIONS } });
  syncFold();
});

describe('with the setting absent or empty', () => {
  it('nothing shows', () => {
    expect(section()).toBe('');
  });
});

describe('a photo that shows no provider looks', () => {
  it('has no section and no switch', () => {
    offered({ offer: { ...OFFER, photo: null } });
    expect(section()).toBe('');
  });
});

describe('with the switch on', () => {
  beforeEach(() => offered({ offer: OFFER }));

  it('shows the header with its switch, the camera and the looks', () => {
    const html = section();
    expect(html).toContain('Provider looks');
    expect(html).toContain('role="switch"');
    expect(html).toContain('aria-checked="true"');
    expect(html).toContain('Presets and LUTs are off while this is on.');
    expect(html).toContain('Camera One');
    expect(html).toContain('Camera Two');
    expect(html).toContain('Matched to this photo');
    expect(html).toContain('Invented Stock');
    expect(html).toContain('Other Stock');
    expect(html).not.toContain('agm-look-dim');
  });

  it('says when the camera is not the photo’s', () => {
    offered({ chosen: { [OPEN]: 'cam2' } });
    expect(section()).toContain('Not this photo’s camera');
  });

  it('shows the spinner only once a look is taking a while', () => {
    const waited = {
      ticket: 1,
      look: 'look1',
      options: {},
      name: 'Invented Stock',
      job: 1,
      then: 'put',
      batch: false,
    } as const;
    offered({ waiting: { [OPEN]: { ...waited, spinning: false } } });
    expect(section()).not.toContain('agm-spin');
    offered({ waiting: { [OPEN]: { ...waited, spinning: true } } });
    expect(section()).toContain('agm-spin');
  });

  it('says on the row why a look could not be made', () => {
    offered({ note: { photo: OPEN, look: 'look2', text: 'looksProviderFailed' } });
    expect(section()).toContain('Couldn’t make this look');
  });

  it('counts a batch, with Cancel', () => {
    offered({ batch: { name: 'Invented Stock', job: 1, done: 12, ended: 12, total: 40, before: [] } });
    const html = section();
    expect(html).toContain('Provider look: 12 of 40');
    expect(html).toContain('Cancel');
  });
});

describe('Preview all', () => {
  const button = (html: string) => html.match(/<button[^>]*>[^<]*Preview[^<]*<\/button>/)?.[0] ?? '';

  it('shows after the options and before the looks, ready to press', () => {
    offered({
      offer: {
        ...OFFER,
        options: [{ id: 'optionA', label: 'Invented choice', default: false, sendsCurrentWb: false }],
      },
    });
    const html = section();
    expect(button(html)).toContain('Preview all');
    expect(button(html)).not.toContain('disabled');
    expect(html.indexOf('Invented choice')).toBeLessThan(html.indexOf('Preview all'));
    expect(html.indexOf('Preview all')).toBeLessThan(html.indexOf('Invented Stock'));
  });

  it('is dimmed and disabled with the switch off', () => {
    offered({ offer: { ...OFFER, on: false } });
    const html = section();
    expect(button(html)).toContain('disabled');
    expect(html).toContain('agm-provider-tools agm-look-dim');
  });

  it('waits for the photo', () => {
    offered({ offer: OFFER });
    const html = renderToStaticMarkup(<ProviderLooks photo={OPEN} ready={false} onTakeOff={() => {}} />);
    expect(button(html)).toContain('disabled');
  });

  it('counts on the button while it runs, and cannot be pressed again', () => {
    offered({ offer: OFFER, previewing: { run: 1, photo: OPEN, job: 1, ended: 3, total: 10 } });
    const html = section();
    expect(button(html)).toContain('Previewing 3 of 10');
    expect(button(html)).toContain('disabled');
  });

  it('has no place in the grid, where there is no one photo', () => {
    Object.assign(useLibraryStore.getInitialState(), { libraryActivePath: OPEN });
    offered({ offer: OFFER });
    const html = renderToStaticMarkup(<ProviderLooks photo={null} ready={false} onTakeOff={() => {}} />);
    Object.assign(useLibraryStore.getInitialState(), { libraryActivePath: null });
    expect(html).toContain('Invented Stock');
    expect(html).not.toContain('Preview all');
  });
});

describe('Delete all previews', () => {
  const WITH_FILES: Offer = { ...OFFER, photo: { camera: 'cam1', files: [], previews: [], clearable: 2, ownWb: null } };
  const button = (html: string) => html.match(/<button[^>]*>[^<]*Delete[^<]*<\/button>/)?.[0] ?? '';

  it('shows beside Preview all, ready to press', () => {
    offered({ offer: WITH_FILES });
    const html = section();
    expect(button(html)).toContain('Delete all previews');
    expect(button(html)).not.toContain('disabled');
    expect(html.indexOf('Preview all')).toBeLessThan(html.indexOf('Delete all previews'));
    expect(html.indexOf('Delete all previews')).toBeLessThan(html.indexOf('Invented Stock'));
  });

  it('is disabled with no look file or preview to clear', () => {
    offered({ offer: OFFER });
    expect(button(section())).toContain('disabled');
  });

  it('counts what the current list does not show', () => {
    offered({ offer: WITH_FILES });
    expect(WITH_FILES.photo?.files).toEqual([]);
    expect(WITH_FILES.photo?.previews).toEqual([]);
    expect(button(section())).not.toContain('disabled');
  });

  it('is on right after Preview all, with only temporary previews', () => {
    const preview = { camera: 'cam1', look: 'look1', path: 'C:/cache/.a.cam1.look1.lut.png' };
    offered({
      offer: { ...OFFER, photo: { camera: 'cam1', files: [], previews: [preview], clearable: 1, ownWb: null } },
    });
    expect(button(section())).not.toContain('disabled');
  });

  it('is disabled with the switch off', () => {
    offered({ offer: { ...WITH_FILES, on: false } });
    const html = section();
    expect(button(html)).toContain('disabled');
    expect(html).toContain('agm-provider-tools agm-look-dim');
  });

  it('waits for the photo', () => {
    offered({ offer: WITH_FILES });
    const html = renderToStaticMarkup(<ProviderLooks photo={OPEN} ready={false} onTakeOff={() => {}} />);
    expect(button(html)).toContain('disabled');
  });

  it('cannot be pressed again while it runs', () => {
    offered({ offer: WITH_FILES, deleting: true });
    expect(button(section())).toContain('disabled');
  });

  it('can stop a running Preview all', () => {
    offered({ offer: WITH_FILES, previewing: { run: 1, photo: OPEN, job: 1, ended: 3, total: 10 } });
    expect(button(section())).not.toContain('disabled');
  });

  it('has no place in the grid, where there is no one photo', () => {
    Object.assign(useLibraryStore.getInitialState(), { libraryActivePath: OPEN });
    offered({ offer: WITH_FILES });
    const html = renderToStaticMarkup(<ProviderLooks photo={null} ready={false} onTakeOff={() => {}} />);
    Object.assign(useLibraryStore.getInitialState(), { libraryActivePath: null });
    expect(html).toContain('Invented Stock');
    expect(html).not.toContain('Delete all previews');
  });

  it('shows nothing with no provider', () => {
    offered({ offer: null });
    expect(section()).toBe('');
  });
});

describe('folding the section', () => {
  const WITH_FILES: Offer = { ...OFFER, photo: { camera: 'cam1', files: [], previews: [], clearable: 2, ownWb: null } };
  const RUNNING = { run: 1, photo: OPEN, job: 1, ended: 3, total: 10 };
  const head = (html: string) => html.match(/<div class="agm-provider-head">.*?<\/div>/)?.[0] ?? '';

  type Node = ReactElement<{ className?: string; role?: string; children?: ReactNode; onClick?(e?: unknown): void }>;

  /** The element in `node` that `is` picks. */
  function found(node: ReactNode, is: (el: Node) => boolean): Node | undefined {
    if (Array.isArray(node)) return node.map((child) => found(child, is)).find(Boolean);
    if (typeof node !== 'object' || node === null || !('type' in node)) return undefined;
    const element = node as Node;
    return is(element) ? element : found(element.props.children, is);
  }

  /** The section's elements, with hooks running as they do in a render. */
  function rendered() {
    let tree: ReactNode;
    const Probe = () => {
      tree = ProviderLooks({ photo: OPEN, ready: true, onTakeOff: () => {} });
      return <>{tree}</>;
    };
    renderToStaticMarkup(<Probe />);
    return tree;
  }

  const clickHead = () => {
    found(rendered(), (el) => el.props.className === 'agm-provider-head')?.props.onClick?.();
    syncFold();
  };

  it('is open at first, with nothing kept for it', () => {
    offered({ offer: WITH_FILES });
    const html = section();
    expect(useUIStore.getState().collapsibleSectionsState).not.toHaveProperty('providerLooksFolded');
    expect(html).toContain('lucide-chevron-down');
    expect(html).toContain('Invented Stock');
    expect(html).toContain('Preview all');
  });

  it('shows only the head and its switch once folded', () => {
    offered({ offer: WITH_FILES });
    folded(true);
    const html = section();
    expect(html).toContain('lucide-chevron-right');
    expect(html).toContain('Provider looks');
    expect(html).toContain('role="switch"');
    for (const gone of ['Invented Stock', 'Preview all', 'Delete all previews', 'Camera One', 'Presets and LUTs']) {
      expect(html).not.toContain(gone);
    }
  });

  it('a click on the head folds it, and another opens it', () => {
    offered({ offer: WITH_FILES });
    clickHead();
    expect(useUIStore.getState().collapsibleSectionsState).toMatchObject({ providerLooksFolded: true });
    expect(section()).not.toContain('Invented Stock');
    clickHead();
    expect(section()).toContain('Invented Stock');
  });

  it('keeps the Edit panel’s sections as they are', () => {
    offered({ offer: WITH_FILES });
    const before = useUIStore.getState().collapsibleSectionsState;
    clickHead();
    expect(useUIStore.getState().collapsibleSectionsState).toMatchObject(before);
  });

  it('the switch works and does not fold it', async () => {
    offered({ offer: WITH_FILES });
    const stopPropagation = vi.fn();
    const button = found(rendered(), (el) => el.props.role === 'switch');
    button?.props.onClick?.({ stopPropagation });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(stopPropagation).toHaveBeenCalled();
    expect(useProviderLooks.getState().offer?.on).toBe(false);
    expect(useUIStore.getState().collapsibleSectionsState).not.toHaveProperty('providerLooksFolded');
  });

  it('cancels nothing, and the head says Preview all is still running', () => {
    offered({ offer: WITH_FILES, previewing: RUNNING });
    clickHead();
    expect(useProviderLooks.getState().previewing).toEqual(RUNNING);
    expect(head(section())).toContain('Previewing 3 of 10');
  });

  it('says nothing in the head while open, as the button shows it', () => {
    offered({ offer: WITH_FILES, previewing: RUNNING });
    expect(head(section())).not.toContain('Previewing');
  });

  it('says nothing in the head with nothing running', () => {
    offered({ offer: WITH_FILES });
    folded(true);
    expect(head(section())).not.toContain('Previewing');
  });

  it('still counts a batch, with its Cancel', () => {
    offered({
      offer: WITH_FILES,
      batch: { name: 'Invented Stock', job: 1, done: 12, ended: 12, total: 40, before: [] },
    });
    folded(true);
    const html = section();
    expect(html).toContain('Provider look: 12 of 40');
    expect(html).toContain('Cancel');
  });

  it('shows nothing with no provider, folded or not', () => {
    offered({ offer: null });
    folded(true);
    expect(section()).toBe('');
  });
});

describe('the groups the provider names', () => {
  const GROUPED: Offer = {
    ...OFFER,
    looks: [
      { id: 'look1', name: 'Invented Stock', group: 'Group B' },
      { id: 'look2', name: 'Other Stock', group: 'Group A' },
      { id: 'look3', name: 'Third Stock', group: 'Group B' },
      { id: 'look4', name: 'Loose Stock', group: null },
    ],
  };
  const ON_PHOTO = {
    lutPath: 'provider-look',
    agProviderLook: {
      camera: 'cam1',
      look: 'look3',
      before: { rawToneRendering: 'dngStandard', rawToneCurve: null, rawToneCurveMode: null },
    },
  };

  /** The adjustments of the open photo, for a server render too. */
  function edited(adjustments: object) {
    Object.assign(useEditorStore.getInitialState(), { adjustments });
    useEditorStore.setState({ adjustments } as never);
  }

  function foldGroup(name: string) {
    useUIStore.setState((s) => ({
      collapsibleSectionsState: { ...s.collapsibleSectionsState, [`providerGroupFolded:${name}`]: true },
    }));
    syncFold();
  }

  const FIRST_ADJUSTMENTS = useEditorStore.getState().adjustments;
  afterEach(() => edited(FIRST_ADJUSTMENTS));

  it('shows the list as it was with no group on it', () => {
    offered({ offer: OFFER });
    const html = section();
    expect(html).not.toContain('agm-look-folder');
    expect(html.indexOf('Invented Stock')).toBeLessThan(html.indexOf('Other Stock'));
  });

  it('heads each group in list order, its looks in theirs, looks with no group first', () => {
    offered({ offer: GROUPED });
    const html = section();
    const order = ['Loose Stock', 'Group B', 'Invented Stock', 'Third Stock', 'Group A', 'Other Stock'];
    const at = order.map((text) => html.indexOf(text));
    expect(at).not.toContain(-1);
    expect(at).toEqual([...at].sort((a, b) => a - b));
  });

  it('comes after Preview all, as the looks did', () => {
    offered({ offer: GROUPED });
    const html = section();
    expect(html.indexOf('Preview all')).toBeLessThan(html.indexOf('Group B'));
  });

  it('a folded group hides its looks and leaves the others', () => {
    offered({ offer: GROUPED });
    foldGroup('Group B');
    const html = section();
    expect(html).toContain('Group B');
    expect(html).not.toContain('Invented Stock');
    expect(html).not.toContain('Third Stock');
    expect(html).toContain('Other Stock');
    expect(html).toContain('Loose Stock');
  });

  it('marks a folded group holding the look on the photo, and no other', () => {
    offered({ offer: GROUPED });
    edited(ON_PHOTO);
    foldGroup('Group B');
    foldGroup('Group A');
    const html = section();
    const mark = html.indexOf('agm-look-folder-on');
    expect(mark).toBeGreaterThan(html.indexOf('Group B'));
    expect(mark).toBeLessThan(html.indexOf('Group A'));
    expect(html.lastIndexOf('agm-look-folder-on')).toBe(mark);
  });

  it('shows the look’s own row when its group is open, with no mark', () => {
    offered({ offer: GROUPED });
    edited(ON_PHOTO);
    const html = section();
    expect(html).toContain('agm-look agm-on');
    expect(html).not.toContain('agm-look-folder-on');
  });

  it('marks nothing with no look on the photo', () => {
    offered({ offer: GROUPED });
    foldGroup('Group B');
    expect(section()).not.toContain('agm-look-folder-on');
  });

  it('marks nothing in the grid, where no photo is open', () => {
    Object.assign(useLibraryStore.getInitialState(), { libraryActivePath: OPEN });
    offered({ offer: GROUPED });
    edited(ON_PHOTO);
    foldGroup('Group B');
    const html = renderToStaticMarkup(<ProviderLooks photo={null} ready={false} onTakeOff={() => {}} />);
    Object.assign(useLibraryStore.getInitialState(), { libraryActivePath: null });
    expect(html).not.toContain('agm-look-folder-on');
  });

  it('Preview all counts the looks of a folded group too', async () => {
    offered({ offer: GROUPED });
    foldGroup('Group B');
    useProviderLooks.getState().previewAll(OPEN, 'cam1', {});
    expect(useProviderLooks.getState().previewing?.total).toBe(GROUPED.looks.length);
  });

  it('with the switch off, dims its headings and leaves them to fold', () => {
    offered({ offer: { ...GROUPED, on: false } });
    const html = section();
    expect(html).toContain('agm-looks-off');
    expect(html).not.toContain('agm-look-folder agm-look-dim');
  });

  it('keeps a look’s note and spinner in its group', () => {
    offered({
      offer: GROUPED,
      note: { photo: OPEN, look: 'look3', text: 'looksProviderFailed' },
    });
    const html = section();
    expect(html.indexOf('Third Stock')).toBeLessThan(html.indexOf('Couldn’t make this look'));
    expect(html.indexOf('Couldn’t make this look')).toBeLessThan(html.indexOf('Group A'));
  });
});

describe('with the switch off', () => {
  it('dims the looks and the camera and says what the switch does', () => {
    offered({ offer: { ...OFFER, on: false } });
    const html = section();
    expect(html).toContain('aria-checked="false"');
    expect(html).toContain('Turn on to use these. Presets and LUTs go off.');
    expect(html).toContain('agm-look-dim');
    expect(html).toContain('agm-provider-camera agm-look-dim');
  });
});

describe('an option the provider offers', () => {
  const OPTION = { id: 'optionA', label: 'Invented <choice>', default: false, sendsCurrentWb: false };
  const checkboxes = (html: string) => html.match(/<input[^>]*type="checkbox"[^>]*>/g) ?? [];

  it('with none offered, no checkbox shows', () => {
    offered({ offer: OFFER });
    expect(checkboxes(section())).toEqual([]);
  });

  it('shows as a checkbox with the provider’s label, as text', () => {
    offered({ offer: { ...OFFER, options: [OPTION] } });
    const html = section();
    expect(checkboxes(html)).toHaveLength(1);
    expect(html).toContain('Invented &lt;choice&gt;');
    expect(html).not.toContain('Invented <choice>');
  });

  it('starts unchecked when its default is off', () => {
    offered({ offer: { ...OFFER, options: [OPTION] } });
    expect(checkboxes(section())[0]).not.toContain('checked');
  });

  it('starts checked when its default is on', () => {
    offered({ offer: { ...OFFER, options: [{ ...OPTION, default: true }] } });
    expect(checkboxes(section())[0]).toContain('checked');
  });

  it('shows the hand choice for this photo', () => {
    offered({ offer: { ...OFFER, options: [OPTION] }, chosenOptions: { [OPEN]: { optionA: true } } });
    expect(checkboxes(section())[0]).toContain('checked');
  });

  it('is dimmed and disabled with the switch off', () => {
    offered({ offer: { ...OFFER, on: false, options: [OPTION] } });
    const html = section();
    expect(checkboxes(html)[0]).toContain('disabled');
    expect(html).toContain('agm-provider-option agm-look-dim');
  });

  it('is not dimmed with the switch on', () => {
    offered({ offer: { ...OFFER, options: [OPTION] } });
    const html = section();
    expect(checkboxes(html)[0]).not.toContain('disabled');
    expect(html).not.toContain('agm-look-dim');
  });

  it('sits after the camera and before the looks', () => {
    offered({ offer: { ...OFFER, options: [OPTION] } });
    const html = section();
    expect(html.indexOf('Matched to this photo')).toBeLessThan(html.indexOf('Invented &lt;choice&gt;'));
    expect(html.indexOf('Invented &lt;choice&gt;')).toBeLessThan(html.indexOf('Invented Stock'));
  });
});

describe('the look on the photo', () => {
  const strip = (there: boolean, canMake: boolean) =>
    renderToStaticMarkup(
      <LookStrip
        canMake={canMake}
        kind="Camera One"
        name="Stock 1.5"
        onMake={() => {}}
        onStrength={() => {}}
        onTakeOff={() => {}}
        strength={100}
        swatch={undefined}
        there={there}
      />,
    );

  it('keeps its whole name, its camera, its Strength and Remake', () => {
    const html = strip(true, true);
    expect(html).toContain('Stock 1.5');
    expect(html).toContain('Camera One');
    expect(html).toContain('Strength');
    expect(html).toContain('Remake');
    expect(html).not.toContain('Look file missing');
  });

  it('says when its file is missing, and offers to make it again', () => {
    const html = strip(false, true);
    expect(html).toContain('Look file missing. Showing Linear without the look.');
    expect(html).toContain('Make it again');
    expect(html).not.toContain('Strength');
  });

  it('offers neither with no provider', () => {
    const html = strip(false, false);
    expect(html).toContain('Look file missing');
    expect(html).not.toContain('Make it again');
    expect(html).not.toContain('Remake');
  });
});
