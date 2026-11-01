import type { ReactElement, ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { initReactI18next } from 'react-i18next';
import i18n from 'i18next';
import type { OfferedLook } from '../ag.generated';
import { AG_NS } from '../locales';
import en from '../locales/en.json';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

const { default: LookGroup, groupsOf } = await import('./LookGroup');
const { useUIStore } = await import('../../store/useUIStore');

const look = (id: string, group: string | null): OfferedLook => ({ id, name: `Stock ${id}`, group });

const FIRST_SECTIONS = { ...useUIStore.getState().collapsibleSectionsState };

/** The store's sections, for a server render too. */
function syncFold() {
  Object.assign(useUIStore.getInitialState(), {
    collapsibleSectionsState: useUIStore.getState().collapsibleSectionsState,
  });
}

const sections = () => useUIStore.getState().collapsibleSectionsState as unknown as Record<string, boolean>;

function group(name = 'Group A', hasOnPhoto = false) {
  return (
    <LookGroup count={2} hasOnPhoto={hasOnPhoto} name={name} onMouseEnter={() => {}}>
      {'A look'}
    </LookGroup>
  );
}

/** Clicks the heading, with hooks running as they do in a render. */
function clickHead(name = 'Group A') {
  let tree: ReactNode;
  const Probe = () => {
    tree = LookGroup({ children: null, count: 2, hasOnPhoto: false, name, onMouseEnter: () => {} });
    return <>{tree}</>;
  };
  renderToStaticMarkup(<Probe />);
  const head = (tree as ReactElement<{ children: ReactElement<{ onClick(): void }>[] }>).props.children[0];
  head.props.onClick();
  syncFold();
}

beforeAll(async () => {
  await i18n.use(initReactI18next).init({ lng: 'en', resources: { en: { [AG_NS]: en } } });
});

beforeEach(() => {
  useUIStore.setState({ collapsibleSectionsState: { ...FIRST_SECTIONS } });
  syncFold();
});

describe('groupsOf', () => {
  it('keeps the groups in the order they first appear and sorts nothing', () => {
    const groups = groupsOf([look('c', 'Group B'), look('a', 'Group A'), look('b', 'Group B'), look('d', 'Group C')]);
    expect(groups.map((g) => g.name)).toEqual(['Group B', 'Group A', 'Group C']);
    expect(groups[0].looks.map((l) => l.id)).toEqual(['c', 'b']);
  });

  it('puts looks with no group first, wherever they are on the list', () => {
    const groups = groupsOf([look('a', 'Group A'), look('b', null), look('c', 'Group A'), look('d', '')]);
    expect(groups.map((g) => g.name)).toEqual([null, 'Group A']);
    expect(groups[0].looks.map((l) => l.id)).toEqual(['b', 'd']);
  });

  it('has no entry for looks with no group when every look has one', () => {
    expect(groupsOf([look('a', 'Group A')]).map((g) => g.name)).toEqual(['Group A']);
  });

  it('with no group at all, is the list as it came', () => {
    const groups = groupsOf([look('b', null), look('a', null)]);
    expect(groups).toEqual([{ name: null, looks: [look('b', null), look('a', null)] }]);
  });

  it('takes a name for a name, whatever it spells', () => {
    const groups = groupsOf([look('a', '__proto__'), look('b', 'constructor'), look('c', '__proto__')]);
    expect(groups.map((g) => g.name)).toEqual(['__proto__', 'constructor']);
    expect(groups[0].looks).toHaveLength(2);
  });
});

describe('a group', () => {
  it('starts open, with nothing kept for it', () => {
    const html = renderToStaticMarkup(group());
    expect(html).toContain('lucide-chevron-down');
    expect(html).toContain('A look');
    expect(Object.keys(sections())).toEqual(Object.keys(FIRST_SECTIONS));
  });

  it('shows its name and how many looks it holds, as the lists below do', () => {
    const html = renderToStaticMarkup(group());
    expect(html).toContain('agm-look-folder');
    expect(html).toContain('Group A');
    expect(html).toContain('<span class="agm-count">2</span>');
  });

  it('a click folds it, and another opens it', () => {
    clickHead();
    let html = renderToStaticMarkup(group());
    expect(html).toContain('lucide-chevron-right');
    expect(html).not.toContain('A look');
    expect(html).toContain('<span class="agm-count">2</span>');
    clickHead();
    html = renderToStaticMarkup(group());
    expect(html).toContain('A look');
  });

  it('is remembered in the state the app saves, each group for itself', () => {
    clickHead('Group A');
    expect(renderToStaticMarkup(group('Group A'))).not.toContain('A look');
    expect(renderToStaticMarkup(group('Group B'))).toContain('A look');
    expect(sections()).toMatchObject(FIRST_SECTIONS);
    expect(Object.keys(sections())).toHaveLength(Object.keys(FIRST_SECTIONS).length + 1);
  });

  it('cannot take a section’s place, or the section’s own fold, by its name', () => {
    for (const name of ['basic', 'providerLooksFolded', '__proto__']) clickHead(name);
    expect(sections()).toMatchObject(FIRST_SECTIONS);
    expect(sections()).not.toHaveProperty('providerLooksFolded');
    expect(Object.getPrototypeOf(sections())).toBe(Object.prototype);
    expect(renderToStaticMarkup(group('__proto__'))).not.toContain('A look');
  });

  it('shows its name as text, cut short by the panel', () => {
    const html = renderToStaticMarkup(group('<b>Group</b>'));
    expect(html).toContain('&lt;b&gt;Group&lt;/b&gt;');
    expect(html).toContain('<span class="min-w-0 truncate">');
  });

  it('says on its heading when, folded, it holds the look on the photo', () => {
    clickHead();
    const html = renderToStaticMarkup(group('Group A', true));
    expect(html).toContain('agm-look-folder-on');
    expect(html).toContain('lucide-film-roll');
    expect(html).toContain('aria-label="On this photo"');
  });

  it('says nothing folded without that look', () => {
    clickHead();
    expect(renderToStaticMarkup(group('Group A', false))).not.toContain('agm-look-folder-on');
  });

  it('says nothing open, where the look’s own row shows it', () => {
    expect(renderToStaticMarkup(group('Group A', true))).not.toContain('agm-look-folder-on');
  });
});
