<!-- source-hash: a5c62330e652 -->
# Themes

After reading this page you will be able to switch between the two bundled themes, override just a few colours, and, if you want, make your own theme from a single TOML file. All theme files live in `~/.tasty/themes/`.

## Bundled themes

| Theme | File | Brightness | Notes |
|------|------|------|------|
| **Catppuccin Mocha** | `~/.tasty/themes/mocha.toml` | Dark | The default theme. If you delete or break it, it is restored to the original on the next launch |
| **Catppuccin Latte** | `~/.tasty/themes/latte.toml` | Light | Created once on first launch. If you delete it, it stays deleted |

Both files are managed by Tasty, so **do not edit them directly** — they are reverted to the original content on every launch. To change colours, use "Changing just a few colours" or "Making your own" below.

## Switching themes

The fastest way is the **theme dot** at the far right of the status bar. Each click toggles between Latte and Mocha. Pressing it while another theme is in use goes to Latte.

To pick from the list:

1. **Settings** (`Ctrl+,`) > **Appearance** > **Theme**.
2. Click one of the cards under **Theme Preset**. Each card shows the name and five representative colours.
3. Press **Save**.

Switching themes resets every colour overridden in the **Colors** tab and the **Tasty** tab. Those values were painted over the previous theme and mean nothing on the new one.

If the theme file named in the settings is missing or unreadable, Tasty starts with Mocha instead and shows a **Theme not found** notice.

## Changing just a few colours

Override colours on top of the current theme without making a theme file. There are three places.

### Appearance > Colors

The **Colors** tab lists every colour of the current theme by group — **Surfaces** · **Overlays** · **Text** · **Accents** · **Terminal-specific** · **ANSI 16**.

1. Untick **Default** on the row you want to change.
2. Pick a colour or type a hex value.
3. **Reset** on a row restores only that colour; **Reset all** at the top restores everything. The number of changes shows as **n changed**.
4. **Save**.

### Appearance > Tasty

Three entries for quickly adjusting only the app chrome.

- **Accent** — The highlight colour used for active markers, focus rings, buttons and so on.
- **Sidebar background**.
- **Active tab indicator** — One of **Underline** / **Fill** / **Dot**.

**Use theme defaults** restores all three at once.

### Appearance > Terminal

Set the terminal Surface's **Focused background** and **Unfocused background** separately. Turn off **Use default** on a row to edit it. Telling a focused terminal from an unfocused one by background colour is Tasty's default behaviour — Mocha uses focused `#000000` / unfocused `#1e1e2e`.

Values overridden this way are stored under `[appearance]` in `~/.tasty/config.toml`.

## Making your own

One file `~/.tasty/themes/<id>.toml` is one theme. The file name (without extension) becomes the theme id, and the theme card in the settings shows `label`. Files in other folders are not read.

**Every colour entry is optional.** Entries you leave out keep the value of the theme that was applied just before. That is why a file of only a few lines is already a theme.

```toml
# ~/.tasty/themes/my-theme.toml
label = "My Theme"      # name shown on the card
is_light = false        # omit to keep the previous theme's value

[palette]
crust    = "#11111b"    # darkest background
mantle   = "#181825"    # sidebar etc.
base     = "#1e1e2e"    # base background
surface0 = "#313244"
surface1 = "#45475a"
surface2 = "#585b70"
overlay0 = "#6c7086"
overlay1 = "#7f849c"
overlay2 = "#9399b2"
text     = "#cdd6f4"
subtext1 = "#bac2de"
subtext0 = "#a6adc8"
placeholder = "#6c7086"

[accent]
blue = "#89b4fa"        # default accent
green = "#a6e3a1"
red = "#f38ba8"
yellow = "#f9e2af"
peach = "#fab387"
mauve = "#cba6f7"
teal = "#94e2d5"
sky = "#89dceb"
lavender = "#b4befe"
flamingo = "#f2cdcd"
pink = "#f5c2e7"
maroon = "#eba0ac"
rosewater = "#f5e0dc"

[terminal]
selection_bg = "#585b70"
vi_cursor_bg = "#b4befe"
search_match_bg = "#f9e2af4d"          # 8 digits = last two are opacity
search_match_active_bg = "#f9e2afb3"

[ansi]
black = "#45475a"
red = "#f38ba8"
green = "#a6e3a1"
yellow = "#f9e2af"
blue = "#89b4fa"
magenta = "#cba6f7"
cyan = "#94e2d5"
white = "#bac2de"
bright_black = "#6c7086"
bright_red = "#f38ba8"
bright_green = "#a6e3a1"
bright_yellow = "#f9e2af"
bright_blue = "#89b4fa"
bright_magenta = "#cba6f7"
bright_cyan = "#89dceb"
bright_white = "#cdd6f4"

[surfaces.terminal]
focused_bg   = "#000000"
focused_fg   = "#cdd6f4"
unfocused_bg = "#1e1e2e"
unfocused_fg = "#a6adc8"

[surfaces.markdown]
focused_bg   = "#11111b"
focused_fg   = "#cdd6f4"
```

Rules:

- Colours use the `#RGB` · `#RRGGBB` · `#RRGGBBAA` formats. With 8 digits, the last two are opacity.
- `[surfaces.<kind>]` holds the focused / unfocused background · text colours per Surface kind. Besides `terminal` · `markdown` you can use kind names registered by plugins, and kinds you do not define are drawn with safe default colours.
- Translucent colours such as hover · selection highlights, and spacing · font sizes, are not in the theme file. They are derived automatically from `is_light`.
- When making a light theme, be sure to write `is_light = true`. The direction of the overlay colours (black tint / white tint) depends on this value.

To apply the file you made, open **Settings** > **Appearance** > **Theme**. The folder is re-read every time this tab is opened, so no restart is needed. When the card appears, click it and **Save**.

To make a variant that changes Mocha only slightly, copy `mocha.toml`, save it under **a different name**, and edit that. Editing `mocha.toml` itself is reverted to the original on the next launch.

## Troubleshooting

| Symptom | What to check |
|------|-----------|
| My theme does not appear in the card list | Whether the file is exactly in `~/.tasty/themes/` and its extension is `.toml`. Close and reopen the settings window |
| **Theme not found** appears right at startup | The file pointed to by `theme = "…"` in `config.toml` is missing or has a syntax error. Fix the file or pick another theme in the settings |
| Some colours remain from the previous theme | This is normal — entries you did not write keep the previous value. Write those entries in the file explicitly |
| Values I changed in the Colors tab disappeared | Switching themes resets overridden values. Move the colours into the theme file instead |

## What to read next

- [Settings](settings.md) — Font · UI scale · opacity in the Appearance tab.
- [A first look](../getting-started/first-look.md) — Where the theme dot is in the status bar.
