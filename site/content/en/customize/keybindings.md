<!-- source-hash: d0d778d2c447 -->
# Keybindings

This page gives the full table of Tasty's default keybindings and shows how to switch presets or change keys one by one to whatever you like. Every keybinding can be changed in **Settings** > **Keybindings**; none is fixed in code.

## Notation

Tasty stores keybindings by the **physical position of the key**. That is why the same setting is pressed differently on the three OSes.

| Name written in settings | Windows · Linux | macOS |
|-------------------|-----------------|-------|
| `ctrl` | Ctrl | Control (⌃) |
| `alt` | Alt | **Command (⌘)** |
| `shift` | Shift | Shift (⇧) |
| `option` | (none) | Option (⌥) |

⌘ on macOS sits in the same place as Alt on Windows · Linux, so the setting value `alt+t` is pressed as `Alt+T` on Windows and `Cmd+T` on macOS. The tables below fill in both columns by this rule.

**Non-Latin keyboards are matched by position too.** Even on a non-Latin layout such as Russian or Greek, keybindings are recognized by the Latin position on the keycap — on a Russian layout, pressing the `H` position (the key that types `Р` in Russian) together with modifiers still triggers the `Ctrl+Shift+H` shortcut. You do not need to re-record shortcuts after switching layouts. The same holds over Markdown and web preview surfaces.

To show the settings screen's notation as the `⌘` `⌥` `⇧` symbols on macOS, change **Alt key display** · **Option key display** · **Shift key display** under **Settings** > **General** > **Display**. The stored values stay the same.

## Default keybindings (Tasty preset)

On a fresh install the **Tasty** preset is applied. When an action has several combinations, all of them work.

### Workspaces · categories

| Action | Windows · Linux | macOS |
|---------|-----------------|-------|
| New Workspace | `Alt+N` | `Cmd+N` |
| Close Workspace | `Alt+Shift+W` | `Cmd+Shift+W` |
| Rename Workspace · change subtitle | `F3` · `F4` | `F3` · `F4` |
| Go to Workspace n | `Alt+1` ~ `Alt+9` | `Cmd+1` ~ `Cmd+9` |
| Next · previous Workspace | `Alt+J` · `Alt+K` | `Cmd+J` · `Cmd+K` |
| Go to category n | `Ctrl+Shift+1` ~ `Ctrl+Shift+0` | `Ctrl+Shift+1` ~ `Ctrl+Shift+0` |
| Next · previous category | `Ctrl+Shift+J` · `Ctrl+Shift+K` | `Ctrl+Shift+J` · `Ctrl+Shift+K` |

Category keybindings work only when **Settings** > **General** > **Workspace categories (folders)** is turned on.

### Panes · Tabs · Surfaces

| Action | Windows · Linux | macOS |
|---------|-----------------|-------|
| Split Pane (left/right / top/bottom) | `Alt+E` / `Alt+Shift+E` | `Cmd+E` / `Cmd+Shift+E` |
| Next · previous Pane | `Ctrl+]` · `Ctrl+[` | `Ctrl+]` · `Ctrl+[` |
| Close Pane | `Ctrl+Shift+W` | `Ctrl+Shift+W` |
| New Tab | `Alt+T` | `Cmd+T` |
| Go to Tab n | `Ctrl+1` ~ `Ctrl+0` | `Ctrl+1` ~ `Ctrl+0` |
| Next · previous Tab | `Ctrl+L` · `Ctrl+H` | `Ctrl+L` · `Ctrl+H` |
| Rename Tab | `F2` | `F2` |
| Close active item (Tab → Pane → Workspace, in that order) | `Ctrl+W` | `Ctrl+W` |
| Restore closed item | `Ctrl+Shift+T` | `Ctrl+Shift+T` |
| Split Surface (left/right / top/bottom) | `Alt+D` / `Alt+Shift+D` | `Cmd+D` / `Cmd+Shift+D` |
| Next · previous Surface | `Alt+]` · `Alt+[` | `Cmd+]` · `Cmd+[` |
| Close Surface | `Alt+W` | `Cmd+W` |
| Convert Surface type (terminal ↔ Markdown, etc.) | `Alt+'` | `Cmd+'` |
| Exit fullscreen stage | `Esc` | `Esc` |

### Terminal · clipboard · explorer

| Action | Windows · Linux | macOS |
|---------|-----------------|-------|
| Copy | `Ctrl+C` · `Alt+C` · `Ctrl+Shift+C` | `Ctrl+C` · `Cmd+C` · `Ctrl+Shift+C` |
| Paste | `Ctrl+V` · `Alt+V` · `Ctrl+Shift+V` | `Ctrl+V` · `Cmd+V` · `Ctrl+Shift+V` |
| Search | `Ctrl+F` · `Alt+F` | `Ctrl+F` · `Cmd+F` |
| vi copy mode | `Ctrl+Shift+Space` | `Ctrl+Shift+Space` |
| Screenshot to clipboard | `Ctrl+Alt+S` | `Ctrl+Cmd+S` |
| Copy path (explorer) | `Alt+Shift+C` | `Cmd+Shift+C` |
| Cut (explorer) | `Ctrl+X` · `Alt+X` | `Ctrl+X` · `Cmd+X` |
| Select all (explorer) | `Ctrl+A` · `Alt+A` | `Ctrl+A` · `Cmd+A` |
| Refresh · go to parent folder (explorer) | `F5` · `Alt+↑` | `F5` · `Cmd+↑` |
| Zoom in · zoom out · reset zoom | `Ctrl+=` · `Ctrl+-` · `Ctrl+0` (`Alt` also works) | `Ctrl+=` · `Ctrl+-` · `Ctrl+0` (`Cmd` also works) |
| Undo · redo (image) | `Ctrl+Z` · `Ctrl+Shift+Z` (`Alt` also works) | `Ctrl+Z` · `Ctrl+Shift+Z` (`Cmd` also works) |

`Ctrl+C` copies when there is selected text; otherwise it interrupts the running program as usual.

### Window · tools

| Action | Windows · Linux | macOS |
|---------|-----------------|-------|
| New window | `Alt+Shift+N` | `Cmd+Shift+N` |
| Open/close Settings | `Ctrl+,` | `Ctrl+,` |
| Open/close notifications | `Ctrl+Shift+I` | `Ctrl+Shift+I` |
| Open/close DAG list | `Ctrl+Shift+G` | `Ctrl+Shift+G` |
| Hide / collapse sidebar | `Ctrl+Shift+B` / `Ctrl+B` | `Ctrl+Shift+B` / `Ctrl+B` |
| Command palette | `Ctrl+Shift+P` · `Alt+Shift+P` | `Ctrl+Shift+P` · `Cmd+Shift+P` |
| Clipboard Viewer (plugin) | `Ctrl+Shift+H` | `Ctrl+Shift+H` |

### Actions with an empty default

The following have no default combination, either to prevent accidents or because they clash with OS shortcuts. Assign one yourself if you need it.

- **Quit** · **Immediate quit** · **Minimize to background** (the Mac preset includes `Cmd+Q` · `Cmd+M`, the Linux preset `Ctrl+Q`)
- Free combinations for **Next tab** · **Previous tab** (left empty because `Ctrl+Tab` clashes with the OS — the numbered switching `Ctrl+L` · `Ctrl+H` works)
- **Open Markdown** · **Open Explorer** · **Convert to Markdown** · **Convert to Explorer**
- **Apply workspace · tab · pane preset**, **Collapse/expand all categories**
- **Minimize window** · **Maximize/Zoom window** · **Close window**
- Open Git Viewer (plugin)

## Presets

**Settings** > **Keybindings** > **Preset** offers four. A preset is only a recommended set and is not tied to the OS — you can use the Mac preset on Windows.

| Preset | Based on | Differences from the Tasty preset |
|--------|------|------------------------|
| **Tasty** (default) | Its own | The tables above. Copy · paste · zoom bundle the conventions of all three OSes |
| **Mac** | iTerm2 · Terminal.app | `Cmd`-centred. Settings `Cmd+,` · notifications `Cmd+Shift+I` · sidebar `Cmd+Shift+B` / `Cmd+B` · command palette `Cmd+Shift+P` · copy/paste `Cmd+C` / `Cmd+V` only · quit `Cmd+Q` · background `Cmd+M`. **Close active item** and **Close Workspace** are empty, and close Pane is `Cmd+Shift+W` |
| **Windows** | Windows Terminal | Pane split `Alt+Shift+E` / `Alt+Shift+D`, Surface split `Alt+D` / `Alt+E` · new window `Ctrl+Shift+N` · copy/paste `Ctrl+C` / `Ctrl+V` only |
| **Linux** | GNOME Terminal | Same as the Windows preset, but copy/paste/cut `Ctrl+Shift+C` / `Ctrl+Shift+V` / `Ctrl+Shift+X` · quit `Ctrl+Q` |

To apply one:

1. Click a preset row in the **Settings** > **Keybindings** > **Preset** list. The preset in use is marked **Active**.
2. In the detail view, check the rows that change in the three-column table of **Action** / **Current** / preset.
3. Press **Apply** at the top right. Nothing is saved to the file yet.
4. Press **Save** at the bottom of the window.

Applying a preset resets every keybinding you changed by hand back to the preset value.

## Changing one keybinding

1. Open **Settings** (`Ctrl+,`) > **Keybindings**. The sub-tabs on the left are divided by what the action targets — **General** · **Workspace** · **Pane** · **Tab** · **Surface** · **Clipboard** · **Zoom** · **Image** · **Explorer** · **Run Scripts** · **Preset** · **Plugins**.
2. Click the key button of the action you want to change; it turns into **Press key combination...**. Press the combination you want.
3. To add another combination to the same action, press **Add binding**.
4. Pressing `Esc` while recording empties that slot.
5. If the combination is already used by another action, the **Shortcut already in use** popup appears. Choosing **Overwrite** removes the combination from the existing action and moves it to this one.
6. Press **Save**. **Cancel** discards everything.

Recording rules:

- Typing keys such as letters · digits · space register only together with at least one modifier (Ctrl/Alt/Shift). `W` alone is ignored.
- Keys such as `F1`~`F12` · `Tab` · `Enter` register without a modifier.
- `Esc` is reserved for "empty the slot" and cannot be recorded as a keybinding. To restore **Exit fullscreen stage**, whose default is `Esc`, reapply a preset.
- When a plugin keybinding (the **Plugins** sub-tab) overlaps a core keybinding, **the plugin's runs first**.

## Changing the numbered-switching rule

"Go to n" · "next/previous" for Tabs · Workspaces · categories are grouped under a **one modifier + one key** rule. Change it at the bottom of the **Tab** sub-tab and the bottom of the **Workspace** sub-tab.

- The **Tab switch modifier** · **Workspace switch modifier** · **Category switch modifier** dropdowns — defaults `Ctrl` · `Alt` · `Ctrl+Shift`. Changing one changes all ten slots on that axis at once.
- The slot buttons **Tab 1:** ~ **Tab 10:** and so on — press a single key without a modifier (**Press a key (no modifier)...**). For example, changing slot 1 to `Q` makes `Ctrl+Q` go to Tab 1, and the number badge shown on the Tab while the modifier is held changes to `Q` as well.
- **Next tab:** · **Previous tab:** and so on — likewise a single key. Defaults are vi-style `L`/`H` (Tabs), `J`/`K` (Workspaces · categories).
- Choosing **Custom** in the dropdown abandons the rule and records a completely different combination for each slot (such as `Ctrl+Alt+1`). No number badge is shown in this mode. Going back to the rule mode resets that axis to its defaults.

## Keybindings do nothing while a popup has focus

While a popup that takes input — the search bar, the file picker, the command palette — **has
focus, no keybinding works at all.** That is deliberate: you would not want `Alt+W` to close a
surface while you are typing into the search bar. If such a popup is merely open and does not
have focus, keybindings work as usual.

There are two ways out.

- **`Esc`** — releases the focused popup. Popups of the kind that close when you click outside
  them close as well; the others stay open and merely lose focus. Other popups that are open
  but not focused are left alone.
- **Click outside the popup** — same result as `Esc`.

While the settings window or the notification panel is open, `Esc` closes that one first.

## Two things that save you from memorising keybindings

- **Modifier key hints** — Hold `Ctrl` · `Alt` · `Shift` and the like for 0.5 seconds or longer (Shift alone: 1.2 seconds) and a list of keybindings starting with that combination appears below the sidebar. It disappears when you let go. Turn it off with **Settings** > **General** > **Accessibility** > **Show modifier key hints**; the panel can be dragged around or resized.
- **Command palette** — `Ctrl+Shift+P` or the **palette** chip in the status bar. Type an action's name and run it with `Enter`. Every action in the Keybindings tab and the global commands of active plugins are searchable.

## Editing the settings file directly

Keybindings are stored in the `[keybindings]` section of `~/.tasty/config.toml`. The values are OS-independent names, so the file can be moved to another OS and used as-is.

```toml
[keybindings]
new_tab = ["alt+t"]
copy = ["ctrl+c", "alt+c", "ctrl+shift+c"]
quit = []                      # empty = no keybinding
tab_switch_modifier = "ctrl"   # combinations such as "ctrl+shift" also work
tab_switch_slot_keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]
tab_switch_next_key = "l"
tab_switch_prev_key = "h"
```

The `+` key itself is written as `ctrl++` or `ctrl+plus`, `-` as `ctrl+-` or `ctrl+minus`, and `=` as `ctrl+=` or `ctrl+equals`. For editing the file in general see [Settings](settings.md).

## What to read next

- [Settings](settings.md) — The settings window structure and `config.toml`.
- [Panes · Tabs · splits](../using/panes-tabs-splits.md) — What the split · move actions in the tables above actually do.
- [Working in the terminal](../using/terminal.md) — Copy mode · search · mouse capture.
- [Lua scripts](scripts.md) — How to register the scripts that show up in the **Run Scripts** subtab.
