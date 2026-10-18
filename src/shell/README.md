# src/shell — the new editor interface

Argentum's own editor interface (DEC-29), beside today's. It is opt-in: open
the app with `?shell=new`. Without it, `src/main.tsx` renders today's app
exactly as before. The layout follows RAWmakase 0.3.3's Develop view (MIT),
itself Lightroom Classic's: from the window's right edge a slim rail, the tool
panels under the histogram and navigator, the Presets / Snapshots / History
panel, then the photo with its toolbar under it, and the filmstrip along the
bottom.

## Rules it is built on

- **No tool-specific interface code.** Panels are built from the engine's
  `ToolDescriptor`s. A new engine tool gets its panel with no change here;
  `tests/newTool.test.tsx` proves it and fails if any file outside `engine/`
  and `locales/` names a tool.
- **One source of truth.** Contract types come from `contract.generated.ts`,
  generated from `src-tauri/engine/src/contract.rs` with its fixtures in
  `contract.fixtures.json`. Never edit either by hand; import from
  `contract.ts`.
- **Edits are patches.** Every change sends the engine an `EditPatch`. Undo
  keeps each step's patch and its inverse, never a copy of the document.
- **Latest wins.** A frame or histogram for an older ticket than one already
  seen is dropped, and so are pixels that arrive after a newer frame.
- **The engine only through the client.** `engine/client.ts` is the one door.
  Until the engine is wired into the app, the door opens on `engine/fake.ts`.

## Layout of the folder

| Path                    | What                                                                                  |
| ----------------------- | ------------------------------------------------------------------------------------- |
| `switch.tsx`            | The door from `src/main.tsx`: `?shell=new` loads the shell, anything else today's app |
| `Shell.tsx`             | The window: top bar, photo and toolbar, side tabs, tool panels, rail, filmstrip       |
| `store.ts`              | The one zustand store: the edit, history, frames, view, interface state               |
| `ShellContext.tsx`      | `useShell(selector)`, the only way components read the store                          |
| `edit.ts`               | The edit document and patches as plain functions: apply, inverse, compose, diff       |
| `contract.ts`           | Re-exports the generated contract types                                               |
| `engine/client.ts`      | `EngineClient`: tools, open, apply, view, frame pixels, events                        |
| `engine/fake.ts`        | The contract fake: the engine's descriptors from the fixtures, its rules for patches  |
| `engine/fakePaint*.ts`  | The fake's pixels, painted in a worker                                                |
| `panels/model.ts`       | Descriptors to sections; value formatting                                             |
| `panels/registry.ts`    | Widgets by setting kind                                                               |
| `panels/PanelStack.tsx` | Sections, tool blocks and setting rows, from descriptors                              |
| `widgets/`              | One widget per kind: `Slider` (float, int), `Toggle` (bool), `Choice` (choice)        |
| `layout/`               | Viewer, toolbar, histogram, navigator, side tabs, rail, filmstrip                     |
| `i18n.ts`, `locales/`   | The shell's own strings, apart from the app's                                         |
| `tests/`                | Vitest: `npm run test:shell`                                                          |

## How a panel appears from a descriptor

1. The store asks the client for `tools()` once and keeps the descriptors.
2. `buildSections` groups them by `section`. Sections come in the order of
   their first tool, tools by `order`, so the pipeline order decides both.
3. `PanelStack` draws a collapsible section per group, a block per tool (its
   label, its origin, a reset), and a row per setting. Settings marked
   `advanced` sit under **More**.
4. Each row asks `widgetFor(spec.kind)` for its widget. A kind with no widget
   yet shows a placeholder row, so its tool still appears.
5. A row reads its value with `useShell((s) => valueOf(s.doc, tool.id, spec))`:
   the edit's value or the descriptor's default. Only that row re-renders when
   it changes.
6. A change calls `setSetting(toolId, key, value)`. The store applies the
   patch at once, records it with its inverse, and sends it to the engine.
   Ticks of one slider drag are one undo step. If the engine refuses a patch,
   that change alone is rolled back and the error is shown.
7. Labels are i18n keys from the descriptor (`tools.<id>.label`,
   `tools.<id>.<key>`, choices `tools.<id>.<key>.<value>`), looked up in
   `locales/en.json`. A key with no string is shown as words made from the key,
   so a tool is readable before it is translated. The locale file is flat,
   because a choice's key sits under its setting's own key.

Adding a widget kind (a curve, a colour) is one component and one
`registerWidget` line in `widgets/index.ts`. Nothing names a tool.

## Contract changes this interface needs

- **Frame pixels.** A `frame` event carries a ticket and a size, and the
  pixels are fetched separately. `EngineClient.frame(ticket)` stands for that
  until the transport is decided.
- **Events name no photo.** The store drops anything older than the photo it
  opened by ticket order alone; a `photoId` on events would make that exact.
- **Thumbnails.** The filmstrip has nothing to show for a photo that is not
  open.
- **Section order and labels.** Sections follow pipeline order and their
  labels live here (`sections.<id>`). If Basic should come before a tool that
  runs earlier, the contract has to say so.
- **Setting dependencies.** Radius applies only with Auto radius off, and
  Threshold only with Auto threshold off; the descriptors cannot say so, so
  both are always live.
- **White balance multipliers.** 0.05 to 20 on a linear rail leaves 1.0 at the
  far left. A scale hint (log) or the engine taking temperature and tint would
  make them usable.

## Running it

```bash
npm run dev          # Vite, then open http://localhost:1420/?shell=new
npm run test:shell   # the shell's tests
```

In a plain browser the shell runs on the contract fake. Today's app needs
Tauri, so it does not load there; the switch keeps the two apart.
