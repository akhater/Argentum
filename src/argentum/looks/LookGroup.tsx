/**
 * The groups of the Provider looks list. Ours.
 *
 * The provider names them and orders them and their looks, so nothing here sorts
 * or knows a name: a group sits where its first look was, and looks with none
 * come first, under no heading. A group starts open. Folded, its heading says
 * when it holds the look on the photo.
 */

import type { ReactNode } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { useUIStore } from '../../store/useUIStore';
import type { OfferedLook } from '../ag.generated';
import { useAgTranslation } from '../locales';
import EditMark from './EditMark';

/** With a group's name, its key beside the Edit panel's sections, as the list's own fold is. */
const FOLDED = 'providerGroupFolded:';

interface Group {
  /** None for the looks with no group. */
  name: string | null;
  looks: OfferedLook[];
}

/** `looks` by group, with what has none first. */
export function groupsOf(looks: OfferedLook[]): Group[] {
  const groups = new Map<string | null, OfferedLook[]>([[null, []]]);
  for (const look of looks) {
    const name = look.group || null;
    const held = groups.get(name);
    if (held) held.push(look);
    else groups.set(name, [look]);
  }
  return [...groups].filter(([, held]) => held.length > 0).map(([name, held]) => ({ name, looks: held }));
}

interface Props {
  children: ReactNode;
  count: number;
  /** The look on the open photo is one of its looks. */
  hasOnPhoto: boolean;
  name: string;
  onMouseEnter: () => void;
}

export default function LookGroup({ children, count, hasOnPhoto, name, onMouseEnter }: Props) {
  const t = useAgTranslation();
  const key = `${FOLDED}${name}`;
  const folded = useUIStore(
    (s) => (s.collapsibleSectionsState as unknown as Record<string, boolean | undefined>)[key] === true,
  );

  const toggle = () =>
    useUIStore.setState((s) => ({
      collapsibleSectionsState: { ...s.collapsibleSectionsState, [key]: !folded },
    }));

  return (
    <div>
      <div className="agm-look-folder" onClick={toggle} onMouseEnter={onMouseEnter}>
        {folded ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        <span className="min-w-0 truncate">{name}</span>
        {folded && hasOnPhoto && (
          <span aria-label={t('looksOnPhoto')} className="agm-look-folder-on" data-tooltip={t('looksOnPhoto')}>
            <EditMark hasProviderLook />
          </span>
        )}
        <span className="agm-count">{count}</span>
      </div>
      {!folded && <div className="agm-look-inner">{children}</div>}
    </div>
  );
}
