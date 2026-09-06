<!-- source-hash: 7776c5b83a56 -->
# Opening files

After reading this page you will know how to open and work with files in non-terminal Surfaces — Explorer · Markdown · image · HTML — and how to open the window that shows git status.

## Ways to open

| Where | How |
|--------|--------|
| Sidebar **Tools** > **Open File…** | Tasty's own file chooser. In a remote Workspace it shows remote files |
| Drag a file onto the window | Drop when **Drop to open** appears |
| Explorer Surface | Double-click a file |
| Terminal | `Ctrl+click`, or select a path and right-click > **Open File** ([Working with the terminal](terminal.md#opening-links-and-paths)) |
| Right-click an empty area of the tab strip | **New Markdown...** · **New Explorer** · **New HTML...** · **New Image** |
| `Alt+'` | Changes the current Surface to another kind ([Changing the kind](panes-tabs-splits.md#changing-the-kind)) |
| CLI | `tasty new tab --pane 1 --type markdown --file README.md` and so on |

The file extension decides which Surface opens it — `.md` is Markdown, image files are image, `.html` is HTML, and a folder is Explorer. If there are several candidates or none, the **Choose file handler** window appears so you can choose, and your choice is kept under **Recent**. Extension mappings and handlers are changed in the **Settings** > **Handler** tab.

A Surface opened from a file is split · moved · closed · restored on restart just like any other Surface. The Tab name becomes the file name.

## Explorer

The file manager built into Tasty. Change a terminal with `Alt+'` > **Explorer** and it opens that terminal's current directory as the root. `tasty new tab --pane 1 --type explorer --path ~/proj` also opens one.

### Screen

- **Left** — the **Files** tree (fixed at the root) and **Favorites** under it.
- **Top** — **Back** · **Forward** · **Up** · **Refresh**, the address bar, and the view switch (**Grid** · **List** · **Detail**).
- **Right** — the items in the current folder. `..` at the top goes to the parent folder. In Detail view, click the **Name** · **Size** · **Modified** · **Type** column headers to sort.
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

### Right-click menu

- **Copy Path** — with several selected, they are joined with line breaks.
- **Copy** · **Cut** · **Paste** · **Paste (into)** — if the name already exists, `(copy)` is appended.
- **Move to Trash** — sends to the OS trash without confirmation. To undo, use the trash.
- **Rename**.
- **Open in System** — opens the folder in the OS file manager.
- **Open in New Tab** — opens one more Explorer Tab in the Pane with that folder as the root.
- **Set as Root** — moves the root of the left tree.
- **Add to Favorites** — gives it a name and puts it in the list at the bottom left. Favorites are shared by all Explorers and saved in `~/.tasty/explorer-favorites.toml`.

The last chosen view mode is remembered and applied to new Explorers too. The Explorer font is set separately in the **Explorer** item under **Settings** > **Appearance**. An Explorer in a remote Workspace is browse-only, and items that change files do not appear.

## Markdown

Renders `.md` files. When the file changes, it redraws automatically within 1 second.

- Tables · checkboxes · footnotes · code highlighting · `mermaid` diagrams · `$…$` math · `> [!NOTE]` callouts (including Obsidian-style `[!tip]-` folding) · frontmatter at the top hidden.
- If there are headings, a collapsible **Table of contents** is attached above the body.
- Hover a code block for the **Copy code** button.
- `Ctrl+F` (macOS `Cmd+F`) while the document has focus — **Find in document**.
- Type another file path in the address bar at the top to move to it. Recently opened files appear as autocompletion.
- Links in the document — other Markdown · files open in a new Tab in the same Pane, and `http(s)://` goes to the browser. Relative paths are relative to the folder the document is in.
- Files over 1MB are asked about once with **Open large file?**.

**New Markdown...** or `Alt+'` > **Markdown** opens the **Open Markdown File** window. Type a path or choose one with **Browse…**.

```sh
tasty markdown recent
tasty markdown reload --surface 5
```

## Image

View PNG · JPEG and so on, and draw simply. **New Image** starts with an empty canvas.

- Toolbar — **Previous image** / **Next image** (within the same folder), **Refresh**, **Edit**, **New image**, zoom **Fit** / `+` / `-`.
- Press **Edit** to choose **Brush** · **Color** and draw on top. Undo and redo with `Ctrl+Z` / `Ctrl+Shift+Z`. **Save** writes a PNG.
- `tasty image paste --surface <ID>` pastes the clipboard image as a floating selection.
- `tasty image list` shows every open image **across all windows**.

```sh
tasty image list
tasty image open --surface 5 shot.png
tasty image export --surface 5 out.png
```

## HTML

Shows a local HTML file or a URL in the OS web view. Press **New HTML...** and type a URL or file path in the **Open HTML** window.

- While loading, **Loading…** is shown; on failure, **Failed to load** and the URL.
- The **HTML** item under **Settings** > **Appearance** — **Default zoom** · **Color scheme** (**Follow theme** / light / dark) · **Allow remote content** · **Sandbox scripts**. Remote content and scripts are blocked by default. It is meant for viewing previews built locally, so to open external sites you need to turn these two items on.

```sh
tasty html open --surface 5 ./dist/index.html
tasty new tab --pane 1 --type html --url http://localhost:3000
```

## Git view

Press **Tools** > **Git** in the sidebar and a window appears showing the repository of the current terminal directory, read-only.

- **Changes** (status) · **Commits** (log) · **Diff** when you click a file.
- Pick another worktree from the **Worktrees** list on the left to view relative to it. The actual checkout does not change.
- **Refresh** picks up external changes. There are no writes such as commit · staging.
- The **Open Git Viewer** shortcut is assigned under **Settings** > **Keybindings** > **Plugins** (no default).

The branch name in the status bar is always shown, independently of this window ([A first look](../getting-started/first-look.md#status-bar)).

## Empty Surface

A slot whose file was closed or whose kind has not been chosen yet remains **Empty**, with a kind-selection button in the middle. Pressing it is the same as the [Changing the kind](panes-tabs-splits.md#changing-the-kind) popup.

The Explorer is built into Tasty; Markdown · image · HTML · Git view are bundled plugins. If you disable a plugin, you cannot open that kind — [Plugins](../plugins/index.md).
