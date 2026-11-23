// The walker's eyes in the page. session.mjs installs `installWalker` in every
// document before the app's first script, then asks it what is on screen:
//
//   controls(scope)  every control a person can reach in the top layer (an open
//                    menu, else an open dialog, else the window), as
//                    "Region > label [kind]", sorted;
//   rawKeys()        visible text that is an i18n key rather than its words;
//   facts()          what the window shows, in a few words, to tell what a
//                    shortcut did;
//   settle()         resolves once no call to the stub is running, no work the
//                    app put off a moment is waiting, and the page has stopped
//                    changing.
//
// "Reachable" is about what is rendered and not hidden by CSS, never about
// where it lands on screen: a font that is a pixel wider on Linux must not
// change the list. So nothing here reads a size, except point(), which says
// where to click.
//
// The function is sent to the page as source text, so it uses nothing outside itself.

export function installWalker(keys) {
  const known = { top: new Set(keys.top), ag: new Set(keys.ag) };
  const INTERACTIVE_TAGS = new Set(['BUTTON', 'INPUT', 'SELECT', 'TEXTAREA', 'SUMMARY']);
  const ROLES = {
    button: 'button',
    link: 'link',
    tab: 'tab',
    menuitem: 'menuitem',
    menuitemcheckbox: 'menuitem',
    menuitemradio: 'menuitem',
    option: 'option',
    treeitem: 'button',
    switch: 'checkbox',
    checkbox: 'checkbox',
    radio: 'checkbox',
    slider: 'slider',
    spinbutton: 'field',
    combobox: 'field',
    textbox: 'field',
  };
  // A right-click area or a drag handle is not a control: menus are walked from their own chapter.
  const HANDLERS = ['onClick', 'onDoubleClick'];
  const FIELDS = 'input:not([type="hidden"]), select, textarea';
  /** The window's parts, by the Modern window's own class names (src/argentum/modern). */
  const REGIONS = [
    ['.agm-top', 'Top bar'],
    ['.agm-left', 'Left column'],
    ['.agm-right', 'Right panel'],
    ['.agm-strip', 'Filmstrip'],
    ['.agm-settings', 'Settings'],
    ['.agm-centre', 'Centre'],
  ];
  const MAX_LABEL = 60;
  /** A timer this short is work the app put off a moment; a longer one (a check every half hour) is not. */
  const SHORT_TIMER_MS = 2000;

  // The app puts work off: the History written 600 ms after the last change, a photo's EXIF read
  // 500 ms after it shows, a status cleared after a second. Until such a timer has run, the step that
  // set it is not over: what it calls belongs to that step however busy the machine, so settle()
  // waits for it as for a call to the stub. Measured over a walk, every timer the app sets is a
  // second or less, or half an hour.
  const wait = window.setTimeout.bind(window);
  const unwait = window.clearTimeout.bind(window);
  const putOff = new Set();
  window.setTimeout = function setTimeout(handler, ms, ...args) {
    const delay = Number(ms ?? 0);
    if (typeof handler !== 'function' || !(delay <= SHORT_TIMER_MS)) return wait(handler, ms, ...args);
    const id = wait(() => {
      putOff.delete(id);
      handler(...args);
    }, delay);
    putOff.add(id);
    return id;
  };
  window.clearTimeout = function clearTimeout(id) {
    putOff.delete(id);
    unwait(id);
  };

  let lastChange = performance.now();
  new MutationObserver(() => (lastChange = performance.now())).observe(document, {
    subtree: true,
    childList: true,
    attributes: true,
    characterData: true,
  });

  const clean = (text) => (text ?? '').replace(/\s+/g, ' ').trim();
  const shorten = (text) => (text.length > MAX_LABEL ? `${text.slice(0, MAX_LABEL - 1)}…` : text);
  const reactProps = (el) => {
    for (const key in el) if (key.startsWith('__reactProps$')) return el[key];
    return null;
  };
  const shown = (el) => el.isConnected && el.checkVisibility({ checkVisibilityCSS: true }) && !el.closest('[inert]');

  function kindOf(el) {
    const role = ROLES[el.getAttribute('role')];
    if (role) return role;
    if (el.tagName === 'INPUT') {
      const type = (el.getAttribute('type') ?? 'text').toLowerCase();
      if (type === 'hidden') return null;
      if (type === 'checkbox' || type === 'radio') return 'checkbox';
      if (type === 'range') return 'slider';
      if (type === 'button' || type === 'submit' || type === 'reset') return 'button';
      return 'field';
    }
    if (el.tagName === 'SELECT' || el.tagName === 'TEXTAREA') return 'field';
    if (INTERACTIVE_TAGS.has(el.tagName)) return 'button';
    if (el.tagName === 'A' && el.hasAttribute('href')) return 'link';
    if (el.isContentEditable && el.getAttribute('contenteditable') !== null) return 'field';
    const props = reactProps(el);
    if (props && HANDLERS.some((name) => typeof props[name] === 'function')) return 'button';
    return null;
  }

  /** Visible text inside `el`, leaving out what belongs to the controls inside it. */
  function ownText(el, controls) {
    const parts = [];
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const parent = node.parentElement;
      if (!parent || !shown(parent)) continue;
      let owner = parent;
      while (owner !== el && !controls.has(owner)) owner = owner.parentElement;
      if (owner === el) parts.push(node.data);
    }
    return clean(parts.join(' '));
  }

  /** The first visible words inside `el`, outside `except` and, for a field's name, outside any button. */
  function firstText(el, except, skipButtons = false) {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = clean(node.data);
      const parent = node.parentElement;
      if (!text || !parent || !shown(parent) || except?.contains(node)) continue;
      if (skipButtons && parent.closest('button') && el.contains(parent.closest('button'))) continue;
      return text;
    }
    return '';
  }

  /** The closest box around a field that holds no other field: the field's own row, label and readout. */
  function unitOf(el) {
    let unit = null;
    for (let box = el.parentElement, depth = 0; box && depth < 4; box = box.parentElement, depth += 1) {
      if (box.querySelectorAll(FIELDS).length > 1) break;
      unit = box;
    }
    return unit;
  }

  /** The text that names a field: its <label>, its placeholder, or the first words of its row. */
  function fieldLabel(el) {
    const labelled = clean([...(el.labels ?? [])].map((label) => label.textContent).join(' '));
    if (labelled) return labelled;
    const placeholder = clean(el.getAttribute('placeholder'));
    if (placeholder) return placeholder;
    const unit = unitOf(el);
    return unit ? shorten(firstText(unit, el, true)) : '';
  }

  /**
   * The slider (components/ui/Slider.tsx) is one control to a person, drawn as three: a label that
   * resets it, a readout that turns into a number field, and the track. The walk lists the track,
   * named by the label; the other two are its parts.
   */
  function sliderParts(found) {
    const parts = new Set();
    for (const el of found) {
      if (el.tagName !== 'INPUT' || el.type !== 'range') continue;
      const unit = unitOf(el);
      if (!unit) continue;
      for (const part of found) {
        if (part !== el && part.tagName !== 'BUTTON' && !part.matches(FIELDS) && unit.contains(part)) parts.add(part);
      }
    }
    return parts;
  }

  function iconOf(el) {
    const svg = el.querySelector('svg[class*="lucide-"]');
    const name = svg && [...svg.classList].find((c) => c.startsWith('lucide-') && c !== 'lucide-icon');
    return name ? `icon ${name.slice('lucide-'.length)}` : '';
  }

  function labelOf(el, controls) {
    const aria = clean(el.getAttribute('aria-label'));
    if (aria) return shorten(aria);
    const kind = kindOf(el);
    if (kind === 'field' || kind === 'slider' || (kind === 'checkbox' && el.tagName === 'INPUT')) {
      return fieldLabel(el) || clean(el.getAttribute('name')) || iconOf(el);
    }
    // A control around other controls (a section's header row, a list row) is named by its first words.
    const around = [...controls].some((other) => other !== el && el.contains(other));
    const text = around ? firstText(el) : ownText(el, controls);
    if (text) return shorten(text);
    // A tooltip the app writes only when a label is cut short depends on the font: never a name.
    if (!el.hasAttribute('data-ag-truncated')) {
      const tip = clean(el.getAttribute('data-tooltip'));
      if (tip) return shorten(tip);
    }
    return shorten(clean(el.getAttribute('title'))) || shorten(clean(el.querySelector('img')?.alt)) || iconOf(el);
  }

  function regionOf(el, layer) {
    if (layer.region) return layer.region;
    for (const [selector, name] of REGIONS) if (el.closest(selector)) return name;
    return 'Window';
  }

  function titleOf(dialog) {
    const id = dialog.getAttribute('aria-labelledby');
    const named = id && document.getElementById(id);
    const heading = named || dialog.querySelector('h1, h2, h3, [role="heading"]');
    const text = clean(heading?.textContent) || clean(dialog.getAttribute('aria-label'));
    return shorten(text || firstText(dialog) || 'untitled');
  }

  /**
   * Modals without a dialog role: a box fixed over the whole window, taking the pointer, with
   * something to press in it (the modals of components/modals that predate the role).
   */
  function overlays() {
    return [...document.querySelectorAll('[class~="fixed"], [style*="fixed"]')].filter((el) => {
      const style = getComputedStyle(el);
      return (
        style.position === 'fixed' &&
        ['top', 'right', 'bottom', 'left'].every((side) => style[side] === '0px') &&
        style.pointerEvents !== 'none' &&
        shown(el) &&
        el.querySelector('button, input, select, textarea, [role]')
      );
    });
  }

  /** The top layer: open menus, else the last open dialog, else the window. */
  function layer() {
    const menus = [...document.querySelectorAll('[role="menu"], .agm-menu')].filter(shown);
    if (menus.length > 0) return { kind: 'menu', roots: menus, region: 'Menu', title: 'Menu' };
    const dialogs = [...document.querySelectorAll('[role="dialog"], [aria-modal="true"]')].filter(shown);
    const others = overlays().filter((el) => !dialogs.some((d) => d.contains(el) || el.contains(d)));
    const top = [...dialogs, ...others].sort((a, b) =>
      a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1,
    );
    const dialog = top.at(-1);
    if (dialog) {
      const title = titleOf(dialog);
      return { kind: 'dialog', roots: [dialog], region: `Dialog "${title}"`, title };
    }
    return { kind: 'window', roots: [document.getElementById('root') ?? document.body], region: null, title: 'Window' };
  }

  let elements = [];

  /**
   * Every control in the top layer, labelled. `scope` (a selector) keeps those inside it, and then
   * entries leave the region out, since the scope names it; `skip` leaves out those inside it.
   */
  function controls({ scope = null, skip = null, deepest = false } = {}) {
    const current = layer();
    const layerRoots = deepest ? current.roots.slice(-1) : current.roots;
    const roots = scope
      ? layerRoots.flatMap((root) => [...(root.matches(scope) ? [root] : []), ...root.querySelectorAll(scope)])
      : layerRoots;
    const found = new Set();
    for (const root of roots.filter(shown)) {
      for (const el of [root, ...root.querySelectorAll('*')]) {
        if (kindOf(el) && shown(el) && !(skip && el.closest(skip))) found.add(el);
      }
    }
    for (const part of sliderParts(found)) found.delete(part);
    elements = [...found];
    const list = elements.map((el, id) => {
      const kind = kindOf(el);
      const region = regionOf(el, current);
      const label = labelOf(el, found);
      const section = sectionOf(el, found, roots);
      const name = section && section !== label ? `${section} > ${label}` : label;
      return {
        id,
        label,
        kind,
        region,
        entry: scope && !current.region ? `${name} [${kind}]` : `${region} > ${name} [${kind}]`,
        disabled: el.disabled === true || el.getAttribute('aria-disabled') === 'true',
        submenu: kind === 'menuitem' && !!el.querySelector('svg.lucide-chevron-right'),
      };
    });
    return { layer: { kind: current.kind, title: current.title }, controls: list.filter((c) => c.label) };
  }

  /**
   * The folding section a control sits in, by its header: a section is a box whose previous sibling
   * is the control that folds it (components/ui/CollapsibleSection.tsx and the panels like it).
   */
  function sectionOf(el, found, roots) {
    for (let box = el.parentElement; box && !roots.includes(box); box = box.parentElement) {
      const header = box.previousElementSibling;
      if (header && found.has(header) && !header.matches(FIELDS)) return shorten(firstText(header));
    }
    return '';
  }

  /** The element `selector` finds (the `index`th), made clickable like a control. */
  function pick(selector, index = 0) {
    const el = [...document.querySelectorAll(selector)].filter(shown)[index];
    if (!el) return null;
    elements.push(el);
    return { id: elements.length - 1, label: selector };
  }

  /** How many menus are open, a submenu counting as one more. */
  function menuCount() {
    const current = layer();
    return current.kind === 'menu' ? current.roots.length : 0;
  }

  /** Close what is open without a key: a press outside, which every menu here listens for. */
  function pressOutside() {
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    document.body.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
  }

  /**
   * Where to click control `id` (from the last controls()), after scrolling it into view if it is not.
   * Only as far as needed: a scroll closes an open context menu.
   */
  function point(id, where = 'centre') {
    const el = elements[id];
    if (!el || !shown(el)) return null;
    el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    const box = el.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) return null;
    const x = Math.round(box.left + box.width / 2);
    // 'bottom' is the empty end of a list or grid, below what it holds.
    const y = Math.round(where === 'bottom' ? box.bottom - 12 : box.top + box.height / 2);
    const hit = document.elementFromPoint(x, y);
    return { x, y, covered: !hit || !(hit === el || el.contains(hit)) };
  }

  /** A click on control `id` from inside the page, for one the pointer cannot reach. */
  function press(id) {
    elements[id]?.click();
  }

  function isRawKey(text) {
    if (/^ag:[\w.]+$/.test(text)) return true;
    if (/ returned an object instead of string/.test(text)) return true;
    if (known.ag.has(text)) return true;
    const dotted = /^([A-Za-z][\w-]*)(\.[\w-]+)+$/.exec(text);
    return !!dotted && known.top.has(dotted[1]);
  }

  /** Visible words that are keys: the text i18next shows for a translation it cannot find. */
  function rawKeys() {
    const found = new Set();
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = clean(node.data);
      if (text && node.parentElement && shown(node.parentElement) && isRawKey(text)) found.add(text);
    }
    for (const el of document.body.querySelectorAll('[aria-label], [title], [placeholder], [data-tooltip], [alt]')) {
      if (!shown(el)) continue;
      for (const name of ['aria-label', 'title', 'placeholder', 'data-tooltip', 'alt']) {
        const text = clean(el.getAttribute(name));
        if (text && isRawKey(text)) found.add(text);
      }
    }
    return [...found].sort();
  }

  /** What the window shows, in words: the view, the open tab, panels, dialogs, menus, messages. */
  function facts() {
    const view = document.querySelector('.agm-view');
    const tab = document.querySelector('.agm-right [role="tab"][aria-selected="true"]');
    const info = document.querySelector('.agm-info-head.agm-on');
    const settings = document.querySelector('.agm-settings .agm-settings-item.agm-on');
    const current = layer();
    return {
      view: !view ? 'none' : view.classList.contains('agm-grid-view') ? 'library' : 'editor',
      photo: clean(document.querySelector('.agm-crumbs .agm-last')?.textContent),
      tab: info ? 'Info' : clean(tab?.textContent),
      settings: settings ? clean(settings.textContent) : '',
      panels: REGIONS.slice(0, 4)
        .filter(([selector]) => document.querySelector(selector))
        .map(([, name]) => name)
        .join(', '),
      layer: current.kind === 'window' ? '' : `${current.kind} ${current.title}`,
      messages: toasts().join(' | '),
      selected: document.querySelectorAll('[data-bench-id="thumbnail"] .bg-accent').length,
      // The first photo's badges in the grid: its rating, flag and colour label show here.
      badges: badgesOf(document.querySelector('[data-bench-id="thumbnail"]')),
      zoom: clean(document.querySelector('.agm-nav-levels .agm-on')?.textContent),
    };
  }

  function badgesOf(el) {
    if (!el) return '';
    const icons = [...el.querySelectorAll('svg[class*="lucide-"]')]
      .filter(shown)
      .map((svg) => [...svg.classList].find((c) => c.startsWith('lucide-') && c !== 'lucide-icon'))
      .filter(Boolean);
    const colours = [...el.querySelectorAll('[style*="background-color"]')].filter(shown).length;
    return clean(`${icons.sort().join(' ')} ${colours ? `${colours} colour` : ''}`);
  }

  /** A message (react-toastify), by its role: App.tsx's toastClassName replaces the library's own class. */
  const TOAST = '.Toastify__toast-container > [role="alert"]';

  /** The words of the messages on screen. */
  const toasts = () => [...document.querySelectorAll(TOAST)].filter(shown).map((t) => clean(t.textContent));

  /** Close every message, as a click on one does. */
  function dismissToasts() {
    for (const toast of document.querySelectorAll(TOAST)) toast.click();
  }

  /** Resolves true once nothing is running or put off and the page has not changed for `quietMs`; false at the timeout. */
  async function settle(quietMs = 150, timeoutMs = 6000) {
    const start = performance.now();
    while (performance.now() - start < timeoutMs) {
      await new Promise((resolve) => wait(resolve, 40));
      // A fade changes no DOM until the modal unmounts at its end (components/modals wait 300 ms):
      // a running transition counts as a change. Spinners run for ever and do not, nor does the bar
      // of a message counting down the 5 s to closing itself: the message is recorded where it shows.
      const fading = document
        .getAnimations()
        .some(
          (a) =>
            a.playState === 'running' &&
            a.effect?.getTiming().iterations !== Infinity &&
            !a.effect?.target?.closest?.('.Toastify__progress-bar'),
        );
      if (fading) lastChange = performance.now();
      // A dialog's code is still loading (components/modals/LazyDialog leaves this marker meanwhile).
      const loading = document.querySelector('[data-dialog-loading]') !== null;
      const busy = (window.__uiWalk?.pending ?? 0) > 0 || putOff.size > 0 || loading;
      if (!busy && performance.now() - lastChange >= quietMs) return true;
    }
    return false;
  }

  /** Resolves with the time since navigation once `selector` matches `count` elements; null at the timeout. */
  async function waitFor(selector, count = 1, timeoutMs = 60000) {
    const start = performance.now();
    while (performance.now() - start < timeoutMs) {
      if (document.querySelectorAll(selector).length >= count) return performance.now();
      await new Promise((resolve) => wait(resolve, 25));
    }
    return null;
  }

  /** Resolves true once at least `count` menus are open; false after `timeoutMs`. */
  async function waitForMenus(count, timeoutMs = 1500) {
    const start = performance.now();
    while (performance.now() - start < timeoutMs) {
      if (menuCount() >= count) return true;
      await new Promise((resolve) => wait(resolve, 25));
    }
    return false;
  }

  window.__walker = {
    controls,
    point,
    press,
    pick,
    menuCount,
    waitForMenus,
    pressOutside,
    rawKeys,
    facts,
    toasts,
    dismissToasts,
    settle,
    waitFor,
  };
}
