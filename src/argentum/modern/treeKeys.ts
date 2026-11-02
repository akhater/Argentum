/**
 * The keyboard in the Modern folder column. Ours, adapted from Shoebox
 * src/components/Sidebar.tsx (`nav`, `onFocus` and `onKey`, @ bccfd70).
 *
 * Up and Down move, Right opens a folder (or steps into it once open), Left
 * closes it (or goes to its parent), Home and End go to the ends, Enter or
 * Space shows the folder. One difference from Shoebox: a folder reached with
 * the arrows is also shown once the key rests, as Explorer's tree does, so
 * browsing needs no Enter. A disk is shown only on Enter, since a whole disk
 * can be a long wait, and so is a folder nothing may read until it is opened,
 * since resting on it would read it. Keys the tree uses stop there, so the
 * grid behind it does not move as well.
 */

import { type FocusEvent, type KeyboardEvent, useEffect, useRef } from 'react';

/** How long the arrows rest on a folder before it is shown. */
const SHOW_AFTER_MS = 250;

interface Row {
  depth: number;
  /** It has folders inside, so Right and Left open and close it. */
  opens: boolean;
  open: boolean;
  /** Reaching it with the arrows does not show it. */
  onlyOnEnter?: boolean;
  selected?: boolean;
}

/** The attributes that make a row reachable from the keyboard. */
export const treeRow = ({ depth, opens, open, onlyOnEnter = false, selected = false }: Row) => ({
  'data-nav': '',
  'data-depth': depth,
  'data-open': opens ? String(open) : undefined,
  'data-only-on-enter': onlyOnEnter ? '' : undefined,
  'data-selected': selected ? '' : undefined,
  tabIndex: -1,
});

/** Marks the control that opens and closes a row, which Right and Left press. */
export const FOLD_CONTROL = { 'data-fold': '' };

/** Spread on the element that scrolls the rows. */
export function useTreeKeys(enabled: boolean) {
  const rowsRef = useRef<HTMLDivElement>(null);
  const showing = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(showing.current), []);

  // One Tab stop for the tree: the row last focused, else the folder shown, else the first.
  useEffect(() => {
    const el = rowsRef.current;
    if (!enabled || !el || el.querySelector('[data-nav][tabindex="0"]')) return;
    (el.querySelector('[data-nav][data-selected]') ?? el.querySelector('[data-nav]'))?.setAttribute('tabindex', '0');
  });

  if (!enabled) return {};

  const onFocus = (e: FocusEvent<HTMLDivElement>) => {
    const row = (e.target as HTMLElement).closest('[data-nav]');
    if (!row) return;
    e.currentTarget.querySelectorAll('[data-nav][tabindex="0"]').forEach((r) => r.setAttribute('tabindex', '-1'));
    row.setAttribute('tabindex', '0');
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    const cur = target.closest<HTMLElement>('[data-nav]');
    if (!cur || target.tagName === 'INPUT' || e.altKey || e.ctrlKey || e.metaKey) return;
    const rows = [...e.currentTarget.querySelectorAll<HTMLElement>('[data-nav]')];
    const i = rows.indexOf(cur);
    const depth = Number(cur.dataset.depth);
    const fold = () => cur.querySelector<HTMLElement>('[data-fold]')?.click();
    const go = (row?: HTMLElement) => {
      if (!row) return;
      row.focus();
      row.scrollIntoView({ block: 'nearest' });
      window.clearTimeout(showing.current);
      if (row.dataset.onlyOnEnter === undefined) {
        showing.current = window.setTimeout(() => {
          if (document.activeElement === row) row.click();
        }, SHOW_AFTER_MS);
      }
    };
    switch (e.key) {
      case 'ArrowDown':
        go(rows[i + 1]);
        break;
      case 'ArrowUp':
        go(rows[i - 1]);
        break;
      case 'Home':
        go(rows[0]);
        break;
      case 'End':
        go(rows[rows.length - 1]);
        break;
      case 'ArrowRight':
        if (cur.dataset.open === 'false') fold();
        else if (cur.dataset.open === 'true' && Number(rows[i + 1]?.dataset.depth) > depth) go(rows[i + 1]);
        break;
      case 'ArrowLeft':
        if (cur.dataset.open === 'true') fold();
        else
          go(
            rows
              .slice(0, i)
              .reverse()
              .find((r) => Number(r.dataset.depth) < depth),
          );
        break;
      case 'Enter':
      case ' ':
        if (target.tagName === 'BUTTON') return;
        window.clearTimeout(showing.current);
        cur.click();
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  return { ref: rowsRef, onFocus, onKeyDown };
}
