# src/shell — the new editor interface

Argentum's own editor interface (DEC-29), beside today's. It is opt-in: open
the app with `?shell=new`. Without it, `src/main.tsx` renders today's app
exactly as before. It has two modes, switched in the top bar or with G and D
as in Lightroom: **Browse**, folders and a grid of their photos, where it
opens when started without a photo; and **Edit**. Edit's layout follows
RAWmakase 0.3.3's Develop view (MIT),
itself Lightroom Classic's: from the window's right edge a slim rail, the tool
panels under the histogram and navigator, the Presets / Snapshots / History
panel, then the photo with its toolbar under it, and the filmstrip along the
bottom. The rail holds the tabs; a tool mode (crop, spot removal, masking)
joins it when a tool declares one.

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
- **Latest wins, and only the photo shown.** A frame or histogram for another
  photo, or for an older ticket than one already seen, is dropped, and so are
  pixels that arrive after a newer frame. A frame is drawn at the placement
  the engine gives it in the view, never stretched to the view.
- **The engine only through the client.** `engine/client.ts` is the one door:
  inside the app it opens on the real engine (`engine/tauri.ts`); in a plain
  browser, in tests or with `?engine=fake`, on `engine/fake.ts`.

## Layout of the folder

| Path                    | What                                                                                  |
| ----------------------- | ------------------------------------------------------------------------------------- |
| `switch.tsx`            | The door from `src/main.tsx`: `?shell=new` loads the shell, anything else today's app |
| `Shell.tsx`             | The window: top bar, Browse or Edit's photo, side tabs, tool panels, rail, filmstrip  |
| `store.ts`              | The one zustand store, made of slices; the photo open and the panels' own state       |
| `historySlice.ts`       | The edit, its undo history, presets and snapshots, and patches on their way           |
| `viewSlice.ts`          | Zoom and centre, renders asked for by ticket, the latest frame and histogram          |
| `view.ts`               | Photo-to-screen maths: fit, the view to ask for, points, parts, frames, device scale  |
| `paths.ts`              | Paths as text: a file's name and folder, folders above it, two spellings compared     |
| `ShellContext.tsx`      | `useShell(selector)`, the only way components read the store                          |
| `edit.ts`               | The edit document and patches as plain functions: apply, inverse, compose, diff       |
| `contract.ts`           | Re-exports the generated contract types                                               |
| `engine/client.ts`      | `EngineClient`: tools, open, apply, view, frame pixels, events                        |
| `engine/fake.ts`        | The contract fake: the engine's descriptors from the fixtures, its rules for patches  |
| `engine/fakeLayout.ts`  | The engine's frame layout, copied, so the fake paints exactly the engine's placement  |
| `engine/fakePaint*.ts`  | The fake's pixels, painted in a worker                                                |
| `engine/tauri.ts`       | The real client, inside the app: `ag` calls, the `ag-engine` event, frame blocks      |
| `engine/AppBar.tsx`     | Inside the app: Open, the photo it was started with, "not saved yet", tick time       |
| `browse/`               | Browse: the folder tree, the photo grid, thumbnails; see below                        |
| `panels/model.ts`       | Descriptors to sections; value formatting                                             |
| `panels/PanelStack.tsx` | Sections, tool blocks and setting rows, from descriptors                              |
| `widgets/`              | `WIDGETS`, one per kind: `Slider` (float, int), `Toggle` (bool), `Choice` (choice)    |
| `layout/`               | Viewer, toolbar, histogram, navigator, side tabs, rail, filmstrip                     |
| `i18n.ts`, `locales/`   | The shell's own strings, apart from the app's                                         |
| `tests/`                | Vitest: `npm run test:shell`                                                          |

## How a panel appears from a descriptor

1. The store asks the client for `sections()` and `tools()` once and keeps
   both.
2. `buildSections` puts the tools in the sections the engine listed, in the
   engine's order, with the engine's labels; tools by `order` within each.
   Empty sections are left out. The interface holds no section order and no
   section names.
3. `PanelStack` draws a collapsible section per group, a block per tool (its
   label, its origin, a reset), and a row per setting. Settings marked
   `advanced` sit under **More**.
4. Each row takes `WIDGETS[spec.kind]`, a record with an entry for every kind
   the contract has, so a kind without a widget does not compile. A float with
   `scale: "log"` moves by ratio, so a multiplier's 1 sits mid-rail.
5. A row reads its value with `useShell((s) => valueOf(s.doc, tool.id, spec))`:
   the edit's value or the descriptor's default. Only that row re-renders when
   it changes. A setting with `enabledWhen` is shown disabled while the other
   setting does not have that value.
6. A change calls `setSetting(toolId, key, value)`. The store applies the
   patch at once, records it with its inverse, and sends it to the engine.
   Ticks of one slider drag are one undo step. The engine answers with the
   edit as it took it (clamped, defaults filled), and that is what is shown.
   If it refuses a patch, that change alone is rolled back and the error's
   message is shown.
7. Labels are i18n keys from the descriptors (`sections.<id>`,
   `tools.<id>.label`, `tools.<id>.<key>`, choices
   `tools.<id>.<key>.<value>`), looked up in `locales/en.json`. A key with no
   string is shown as words made from the key, so a tool is readable before it
   is translated. The locale file is flat, because a choice's key sits under
   its setting's own key.

Adding a widget kind (a curve, a colour) is one component and one entry in
`WIDGETS` (`widgets/index.ts`). Nothing names a tool.

## Browse (`browse/`)

A file browser, not a library (DEC-30): it lists folders and shows their
photos, and remembers only favourite and recent folders, in Argentum's
preferences. No catalog, no ratings, no last photo: started without one, the
window opens in Browse at the folders remembered.

| Path                  | What                                                                                    |
| --------------------- | --------------------------------------------------------------------------------------- |
| `browse.generated.ts` | The types, generated from `src-tauri/src/mods/browse/contract.rs`, with fixtures beside |
| `client.ts`           | `BrowseClient`: places, folders, list, want, thumb, favourites, recent, events          |
| `tauri.ts`            | The real client: `browse_*` calls through `ag`, the `ag-browse` event                   |
| `fake.ts`             | The browse fake: a made-up disk, 1,200 photos in one folder, painted thumbnails         |
| `browseSlice.ts`      | The store's part: tree, folder, its photos sorted, the mode, what is picked             |
| `thumbs.ts`           | The thumbnails the page holds, as object URLs, within 48 MB                             |
| `FolderTree.tsx`      | Favourites, recent, then Pictures, home and the drives, each folder read when opened    |
| `PhotoGrid.tsx`       | The grid, drawn only as far as it shows; the photos on screen asked for first           |
| `BrowseView.tsx`      | Tree and grid side by side; the folders remembered before one is picked                 |

A folder's photos come as `listed` events in parts, and the newest listing
stops older ones. Thumbnails are asked for with `want(paths)`, the photos on
screen first and then a few rows ahead; each call replaces the one before,
so scrolling or changing folder never waits behind thumbnails no longer
wanted. A `thumb` event says one is ready and `thumb(path)` fetches its
bytes. The app makes them from the JPEG the camera embeds in the RAW, never
by developing it (Shoebox's reader, copied), and keeps them in memory and on
disk (`src-tauri/src/mods/browse/`). Edit's filmstrip is the folder browsed.

## Contract gaps still open

- **Frame pixels.** A `frame` event carries a ticket, a photo and a size, and
  the pixels are fetched separately. `EngineClient.frame(ticket)` stands for
  that until the transport is decided.
- **Photos the engine cannot open.** Browse lists JPEG, TIFF and PNG files
  beside RAWs; the engine decodes RAWs only so far, so opening one of those
  in Edit says so.

## Running it

```bash
npm run start:shell  # the app, straight into the shell, on the real engine
npm run start:shell -- C:\photos\IMG_0001.CR3   # the same, opening that RAW
npm run dev          # Vite, then open http://localhost:1420/?shell=new
npm run test:shell   # the shell's tests
```

Inside the app the shell edits in memory only: nothing is saved, and
`.agdata` files are never read or written. `npm run tauri dev` starts
today's interface as before.

In a plain browser the shell runs on the contract fake and the browse fake.
Today's app needs
Tauri, so it does not load there; the switch keeps the two apart.
