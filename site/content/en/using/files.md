<!-- source-hash: 240c587d99ba -->
# Opening files

Read a README or check an image beside your terminal. Tasty can open an Explorer, Markdown documents, images, and HTML pages, with a Git viewer for reviewing changes.

## Ways to open

| Where | How |
|--------|--------|
| Sidebar **Tools** > **Open File…** | Tasty's own file chooser. It opens in the folder of the terminal or Explorer you are looking at, and in a remote Workspace it shows remote files |
| Drag a file onto the window | Drop when **Drop to open** appears |
| Explorer Surface | Double-click a file |
| Terminal | `Ctrl+click`, or select a path and right-click > **Open File** ([Working with the terminal](terminal.md#opening-links-and-paths)) |
| Right-click an empty area of the tab strip | **New Markdown...** · **New Explorer** · **New HTML...** · **New Image** |
| `Alt+'` | Changes the current Surface to another kind ([Changing the kind](panes-tabs-splits.md#changing-the-kind)) |
| CLI | `tasty new tab --pane 1 --type markdown --file README.md` and so on |

The file extension decides which Surface opens it — `.md` is Markdown, image files are image, `.html` and `.svg` are HTML, and a folder is Explorer. When nothing settles which one opens it, the **Open file with…** window appears so you can choose, and your choice is kept under **Recent**. Extension mappings and handlers are changed in the **Settings** > **Handler** tab.

The **Open file with…** window lists the file path, detected format, and available handlers. Each handler has an icon, a name, and its origin (**built-in** · **you** · **plugin**).

**Recent** shows previously used handlers and when you used them: just now, minutes ago, hours ago, yesterday, or days ago. After a week, it shows a date such as `2026-09-13`. The display updates if the window stays open across midnight.

One click selects a handler; a double-click opens the file. This choice applies **only this time**. It does not register a default for the file type, so the chooser can appear again for the same file. Set a default under **Settings** > **Handler**. Press `Esc` or **Cancel** to close the chooser.

A Surface opened from a file is split · moved · closed · restored on restart just like any other Surface. The Tab name becomes the file name.

Drag the file chooser by its top row, the one with the title and ✕. Closing it with ✕, **Cancel**, or `Esc` opens nothing.

In the file chooser, the list has a row of column names for name, size and modification date, and the selected row carries a coloured bar on its left edge. Long file names end with `…` so they do not overlap the size and modification date. This only changes the display; selecting or opening an entry still uses its full name.

In the list, a single click selects the row, file or folder; a double click enters a folder or opens a file. Pressing **Open** with a folder selected enters that folder — the way down by keyboard. When choosing where to save, a folder cannot be the target, so selecting one leaves the name field as it is and shows a grey line below saying so. When saving, double-clicking a file only fills in its name and does not save — this leaves you the chance to read the overwrite warning.

A file you open yourself, by any of the ways above, opens in a new tab and **switches to it.** When an AI agent opens one for you, the tab is added and the tab you were on stays selected, so the screen you are working on does not change under you.

When an opening request specifies its source surface, switching windows while it is pending keeps the result in a new tab in that surface’s pane. Opening a file from the Explorer while you are looking at another pane still puts it in the Explorer’s pane. Closing the source does not open the file in another window.

## Explorer

The file manager built into Tasty. Change a terminal with `Alt+'` > **Explorer** and it opens that terminal's current directory as the root. `tasty new tab --pane 1 --type explorer --path ~/proj` also opens one.

### Screen

- **Left** — the **Files** tree (fixed at the root) and **Favorites** under it.
- **Top** — **Back** · **Forward** · **Up** · **Refresh**, the address bar, and the view switch (**Grid** · **List** · **Detail**).
- **Right** — the items in the current folder. `..` at the top goes to the parent folder. In Detail view, click the **Name** · **Size** · **Date modified** · **Type** column headers to sort.
- You can keep several **New tab**s inside a Surface and view folders separately. These are separate from the Pane's Tabs.

Click the address bar to type a path directly; recently visited folders appear as autocompletion. Go with `Enter` or **Go**. The left tree stays fixed at the root, but the right list can go anywhere.

### Shortcuts (when the Explorer has focus)

| Action | Shortcut |
|---------|--------|
| Refresh | `F5` |
| Go to parent folder | `Alt+↑` |
| Select all | `Ctrl+A` · `Alt+A` |
| Copy path | `Alt+Shift+C` |
| Copy / cut / paste | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` (the same bindings as the terminal) |

Click to select, `Ctrl+click` to add, `Shift+click` to select a range.

### Type to find

With the list focused, type the first letters of a file name and the first item starting with those letters is selected and scrolled into view. This works the same in Grid, List and Detail view. Pressing the same letter again steps through the candidates, wrapping to the first after the last one. Typing different letters in a row turns it into a prefix search, such as `co` then `con`. Case is ignored, and pausing for about a second starts a new search. If no name matches, the selection stays where it was.

While you are editing the address bar or a popup is open, the letters go there instead. If you have several Explorers open, only the focused one responds.

If you have bound a shortcut to a single letter with no modifier, that letter acts only as the shortcut and is not used here. The default settings have no such shortcut.

### Right-click menu

Renaming refuses to overwrite an existing entry and reports the failure. Copying preserves symbolic links as links. You cannot paste a folder into itself or one of its descendants through another path. Failed file operations display an error notification.

- **Copy Path** — with several selected, they are joined with line breaks.
- **Copy** · **Cut** · **Paste** · **Paste (into)** — if the name already exists, `(copy)` is appended.
- **Move to Trash** — sends to the OS trash without confirmation. To undo, use the trash.
- **Rename**.
- **Open in System** — opens the folder in the OS file manager.
- **Open in New Tab** — opens one more Explorer Tab in the Pane with that folder as the root.
- **Set as Root** — moves the root of the left tree.
- **Add to Favorites** — gives it a name and puts it in the list at the bottom left. Favorites are shared by all Explorers and saved in `~/.tasty/explorer-favorites.toml`.

The last chosen view mode is remembered and applied to new Explorers too. The Explorer font is set separately in the **Explorer** item under **Settings** > **Appearance**. In an Explorer in a remote Workspace, items that change files do not appear. Double-clicking a markdown file opens a tab for it on the remote computer; if the type's default handler can't be used remotely, only the handlers that can are offered, and a type with no such handler only shows a notice.

## Markdown

Renders `.md` files. When the file changes, it redraws automatically within 1 second.

- Tables · checkboxes · footnotes · code highlighting · `mermaid` diagrams · `$…$` math · `> [!NOTE]` callouts (including Obsidian-style `[!tip]-` folding) · frontmatter at the top hidden.
- If there are headings, a collapsible **Table of contents** is attached above the body. Clicking an entry moves down to that heading within the document you are already reading — and clicking the same entry again moves there again.
- Hover a code block for the **Copy code** button.
- `Ctrl+F` (macOS `Cmd+F`) while the document has focus — **Find in document**.
- Type another file path in the address bar at the top to move to it. Recently opened files appear as autocompletion. Files opened in other windows of the same Tasty instance are included, with up to 10 entries in most recently opened order.
- Links in the document — other Markdown · files open in a new Tab in the same Pane, and on Linux Tasty switches to it (on macOS the new Tab is only added behind the current one), and `http(s)://` goes to the browser. Relative paths are relative to the folder the document is in. If no handler can open a linked file, the **Open file with…** window appears so you can choose (in a remote Workspace it lists only handlers that can open the file on the remote, and a notice appears when there are none). Links that point inside the same document, such as `#heading`, and footnote numbers and their back arrows only move to that spot.
- Files over 1MB are asked about once with **Open large file?**.

**New Markdown...** or `Alt+'` > **Markdown** opens the **Open Markdown File** window. Type a path or choose one with **Browse…**. Browse starts in the current folder of the terminal you are looking at, and a relative path you type is opened from that folder too. The window appears centered over that surface, and it hides while you switch to another workspace or tab and comes back as you left it when you return. The file chooser opened by Browse hides and returns with it, keeping your selected files and current folder. While hidden, it does not block clicks or keyboard input on the other screen.

```sh
tasty markdown recent
tasty markdown reload --surface 5
```

## Image

View PNG, JPEG, and other images, or add drawings to them. **New Image** starts with an empty canvas. When the file changes outside Tasty, it is re-read automatically within 1 second — except **while you are editing**, where it is deferred until you leave edit mode (so the picture underneath your strokes does not change).

- Toolbar — **Previous image** / **Next image** (within the same folder), **Refresh**, **Edit**, **New image**, zoom **Fit** / `+` / `-`.
- Press **Edit** to choose **Brush** · **Color** and draw on top. Undo and redo with `Ctrl+Z` / `Ctrl+Shift+Z`. **Save** writes a PNG.
- `tasty image paste --surface <ID>` pastes the clipboard image as a floating selection.
- `tasty image list` shows every open image **across all windows**.
- `tasty image reload --surface <ID>` re-reads the file right now instead of waiting.

```sh
tasty image list
tasty image open --surface 5 shot.png
tasty image export --surface 5 out.png
```

## HTML

Shows a local HTML file or a URL in the OS web view. Press **New HTML...** and type a URL or file path in the **Open HTML** window.

- While loading, **Loading…** is shown; on failure, **Failed to load** and the URL.
- The **HTML** item under **Settings** > **Appearance** — **Default zoom** · **Color scheme** (**Follow theme** / light / dark) · **Allow remote content** · **Sandbox scripts**. Remote content and scripts are blocked by default. It is meant for viewing previews built locally, so to open external sites you need to turn **Allow remote content** on and **Sandbox scripts** off.

### Documents with blocked scripts

With **Sandbox scripts** on, viewing a document that contains scripts shows a **Scripts in this document are blocked** notice above the page. The notice does not cover the page; it pushes the page down by its height. When the HTML view is narrow, the button moves to the line below the description.

- **Allow for this document** — turns scripts on for this document only and reloads it once. Navigating to another document or restarting Tasty blocks them again. The tab of an allowed document gets a script file icon.
- While the view is loading a new document, **Allow for this document** can't be pressed. Hovering it shows **Available when the document finishes loading**. When a document you opened yourself (a link, going back, or reloading) has scripts, the button comes back; when it has none, the notice goes away. A document an agent opened shows the notice when you look at that tab again.
- **×** at the top right of the notice — closes the notice without allowing. A lock icon stays after the tab title; press the lock to show the notice again. Pressing the lock does not switch tabs.
- A document opened by an agent, or reopened by session restore, shows the notice when a person looks at that tab. Agents cannot allow scripts; they can only query the state.
- While **Failed to load** is shown because the document could not be loaded, the notice and the tab's lock or script icon are removed. When a reloaded document opens, they are decided again for that document.

```sh
tasty html open --surface 5 ./dist/index.html
tasty new tab --pane 1 --type html --url http://localhost:3000
tasty surface html-script --surface 5   # script detection, allowance and notice state
```

## Git view

Press **Tools** > **Git** in the sidebar and a window appears showing the repository of the current terminal directory, read-only.

- **Changes** (status) · **Commits** (log) · **Diff** when you click a file.
- Pick another worktree from the **Worktrees** list on the left to view relative to it. The actual checkout does not change.
- **Refresh** picks up external changes. There are no writes such as commit · staging.
- The **Open Git Viewer** shortcut is assigned under **Settings** > **Keybindings** > **Plugins** (no default).

The status bar can show the current repository’s branch independently of this window ([A first look](../getting-started/first-look.md#status-bar)).

## Empty Surface

A slot whose file was closed or whose kind has not been chosen yet remains **Empty**, with a kind-selection button in the middle. Pressing it is the same as the [Changing the kind](panes-tabs-splits.md#changing-the-kind) popup.

The Explorer is built into Tasty; Markdown · image · HTML · Git view are bundled plugins. If you disable a plugin, you cannot open that kind — [Plugins](../plugins/index.md).
