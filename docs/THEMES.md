# Making themes for Venuta

A theme changes everything about how Venuta looks: colours, glass, corners, type, the logo, a
picture behind everything, and effects like grain or scanlines. The layout stays Venuta's, so
every theme works on every screen and keeps working after updates.

There are three ways to make one:

1. **In the Theme Studio** (Settings, Look and feel, Your themes, **New theme**). Start from the
   theme in use, the album playing, or a few colours. The whole app is the preview.
2. **By hand**: a folder with a `theme.json`, in your library's `.venuta\themes`. Venuta
   reloads it each time you save the file.
3. **With an AI assistant**: Settings, Look and feel, **Copy a prompt for an AI assistant**.
   Paste it into the assistant, describe your theme, save what it writes as `theme.json`, and
   import it with **Import…**.

## Where themes live

Each theme is a folder in your library's data folder:

```
<your library>\.venuta\themes\midnight-sakura\
    theme.json          what the theme says (required)
    extra.css           its expert stylesheet (optional)
    art\background.jpg  pictures it uses (optional)
    fonts\ui.woff2      fonts it brings (optional)
    .history\           every earlier version of theme.json, kept by Venuta
```

The folder's name is the theme's id: lowercase letters, digits and dashes. Removing a theme in
Venuta moves its whole folder to your library's **Set aside**, so nothing is ever lost.

## A theme, small

A theme starts as one of Venuta's built-in themes (`extends`) and writes only what differs.
This is a complete theme:

```json
{
  "format": 1,
  "id": "plum-night",
  "name": "Plum Night",
  "extends": "venuta",
  "modes": ["dark"],
  "accent": { "color": "#ff8fb8" },
  "tokens": {
    "app-bg": "#120a18",
    "ink": "#f7eaf2",
    "glass": "rgb(255 214 236 / 0.05)",
    "np-bg": "rgb(40 22 50 / 0.7)"
  }
}
```

Everything it doesn't say comes from Lumen, the theme it extends. It also keeps what Lumen
*does*: its waveform seek bar, its glass mini player, its album background. A theme built on
Neo-Tokyo keeps Neo-Tokyo's light-trail seek bar and city scene, and so on.

## Everything a theme.json can say

| Key | What it is |
|---|---|
| `format` | `1`. A newer Venuta reads every older format. |
| `id`, `name`, `author`, `description` | Its id (the folder's name), and what people see. |
| `extends` | The built-in theme it starts from (see the table below). |
| `modes` | `["dark"]`, `["light"]` or `["dark", "light"]`. With one, the theme always shows in it. |
| `accent` | The key colour: `{"color": "#ff8fb8"}`, `{"from": "album"}` (the cover of the song playing), or `{"from": "base"}` (whatever the base theme does). |
| `tokens` | Values for every mode, by name (below). |
| `dark.tokens`, `light.tokens` | Values for one mode only, over `tokens`. |
| `fonts` | `{"ui": …, "display": …, "readout": …}`: `"builtin:Inter Variable"`, or a font file in the theme (`"fonts/ui.woff2"`; .woff2, .woff, .ttf or .otf, up to 2 MB). |
| `background` | A picture behind everything: `{"image": "art/background.jpg", "dim": 0.35, "blur": 0}`. Dim is 0 to 0.95, blur 0 to 60 px. With a picture, the album background steps aside. |
| `effects` | From the effects menu: `{"scene": "base", "grain": 0.3, "scanlines": 0, "halftone": 0, "hex": 0, "wires": false, "rain": false, "trails": false, "split": false, "wipe": false, "lcd": false}`. Numbers are 0 to 1. Moving effects rest when motion is reduced. |
| `mark` | The logo: `{"color": "accent", "glow": 0.6}`. Colour is `accent`, `ink` or a colour. |
| `css` | Its expert stylesheet's file name, usually `extra.css`. |
| `studio` | The Theme Studio's own notes (its simple choices). Leave it as it is. |

### The effects

| Effect | What it is |
|---|---|
| `scene` | A whole built-in scene: Neo-Tokyo's city, Thermoptic's data rain, Sync Ratio's hex grid, Layer Zero's wires and static, Bounty's halftones and colour wipe, Skin '99's brushed metal. `base` keeps the base theme's; `none` turns it off. |
| `grain` | Film grain over everything. |
| `scanlines` | CRT lines over everything. |
| `halftone` | Print dots in a corner. |
| `hex` | A hexagon grid behind everything. |
| `wires` | Telephone wires across the top. |
| `rain` | Falling columns of digits. |
| `trails` | Light trails sweep across when the song changes. |
| `split` | A short colour split when the song changes. |
| `wipe` | Colour blocks wipe the player bar when the song changes. |
| `lcd` | An LCD spectrum beside the player's controls. |

## Every value

Values are plain CSS: colours (`#rrggbb`, `rgb(r g b / a)`, `oklch(…)`), gradients, filters
(`blur(24px) saturate(1.5)`), shadows and sizes. A value may use another with
`var(--name)`, for example `"bar": "var(--glass)"`.

<!-- VALUES: written by scripts/theme-docs.mjs -->

### Colours

| Value | Kind | What it changes |
|---|---|---|
| `ink` | colour | Text and icons |
| `accent` | colour | The key colour: Play, the chosen tab, progress |
| `on-accent` | colour | Text and icons on the key colour |
| `accent-ink` | colour | The key colour as text (links, "In use") |
| `accent-2` | colour | A second colour some themes use for highlights |
| `problem` | colour | Problems (Health, errors) |
| `fix` | colour | Things to fix (warnings) |
| `good` | colour | Good news (done, healthy) |
| `note` | colour | Notes and hints |
| `block` | colour | The colour block (themes built on Bounty Session) |

### Page

| Value | Kind | What it changes |
|---|---|---|
| `app-bg` | colour | The window behind everything |
| `app-image` | gradient, or none | A gradient over the window colour |
| `solid` | colour | Solid fill where nothing may show through |
| `backdrop-opacity` | number 0 to 1 | How much of the album background shows (0 to 1) |
| `backdrop-filter` | filter, or none | How the album background is treated (a filter) |
| `art-tint` | filter, or none | How album art inside pages is tinted (a filter) |
| `scrim` | colour | The shade behind dialogs |
| `vignette` | gradient, or none | Darkening at the window edges |

### Panels

| Value | Kind | What it changes |
|---|---|---|
| `glass` | colour | Panels and cards on a page |
| `edge` | colour | Panel outlines |
| `edge-strong` | colour | Outlines that need to stand out |
| `highlight` | colour | The lit top edge of a panel |
| `shadow` | shadow, or none | Panel shadow |
| `shadow-e3` | shadow, or none | Shadow of things that float (dialogs) |
| `blur` | blur, or none | Blur behind panels that have one |

### Over content

| Value | Kind | What it changes |
|---|---|---|
| `popover` | colour | Menus, the palette, drawers and dialogs |
| `blur-over` | blur, or none | Blur behind them |

### Bars

| Value | Kind | What it changes |
|---|---|---|
| `bar` | colour | Bars pinned over scrolling content |
| `bar-blur` | blur, or none | Blur behind them |
| `bar-edge` | colour | Their outline |

### Rail

| Value | Kind | What it changes |
|---|---|---|
| `rail-bg` | colour | The side rail |
| `rail-blur` | blur, or none | Blur behind the rail |
| `rail-edge` | colour | The rail outline |
| `rail-shadow` | shadow, or none | The rail shadow |
| `pill-bg` | colour | The chosen place on the rail |
| `pill-ink` | colour | Its text and icon |
| `pill-shadow` | shadow, or none | Its edge or glow |

### Player bar

| Value | Kind | What it changes |
|---|---|---|
| `np-bg` | colour | The player bar |
| `np-blur` | blur, or none | Blur behind it |
| `np-edge` | colour | Its outline |
| `np-shadow` | shadow, or none | Its shadow |

### Accent material

| Value | Kind | What it changes |
|---|---|---|
| `accent-fill` | colour or gradient | What buttons in the key colour are made of |
| `accent-sheen` | shadow, or none | Their light (an inset shadow) |
| `accent-blur` | blur, or none | Blur behind see-through ones |

### Shape

| Value | Kind | What it changes |
|---|---|---|
| `r-rail` | size | Corner of the rail |
| `r-np` | size | Corner of the player bar and covers |

### Type

| Value | Kind | What it changes |
|---|---|---|
| `font-ui` | font list | Everyday text |
| `font-display` | font list | Titles |
| `font-readout` | font list | Times and numbers |
| `title-weight` | 100 to 900 | Title weight (100 to 900) |
| `title-tracking` | letter spacing | Title letter spacing |
| `title-case` | none, uppercase, lowercase | Titles in capitals or not |
| `label-case` | none, uppercase, lowercase | Small labels in capitals or not |

### Stage

| Value | Kind | What it changes |
|---|---|---|
| `stage-bg` | colour | The inset stage (themes built on Glasshouse) |
| `stage-edge` | colour | Its outline |
| `stage-radius` | size | Its corner |
| `stage-margin` | size | Its distance from the window edge |

### Built-in themes to build on

| `extends` | Theme |
|---|---|
| `venuta` | Lumen: Light through glass, over your album art, in the colour of what you play. |
| `green-room` | Green Room: Black panels, a green Play and big bold titles. After Spotify. |
| `orchard` | Orchard: Quiet greys, a red-pink key and a floating player. After Apple Music. |
| `redline` | Redline: Pitch black, white pills and a red line. After YouTube Music. |
| `glasshouse` | Glasshouse: Dark glass, an inset stage tinted by each cover. After Cider. |
| `neo-tokyo` | Neo-Tokyo: Red light-trails through a neon night city. After Akira (1988). |
| `thermoptic` | Thermoptic: Rain-damp teal city and terminal-green data. After Ghost in the Shell. |
| `sync-ratio` | Sync Ratio: Emergency orange, cockpit purple, serif title cards. After Evangelion. |
| `layer-zero` | Layer Zero: Blue-grey stillness and shadows of red static. After Serial Experiments Lain. |
| `bounty` | Bounty Session: Ink on paper and colour blocks, cut to a horn section. After Cowboy Bebop. |
| `skin99` | Skin '99: Brushed gunmetal, bevels and a green LCD. After the skinned players of 1999. |

### Fonts you can name

`builtin:Sora Variable`, `builtin:Inter Variable`, `builtin:Figtree Variable`, `builtin:Roboto Variable`, `builtin:Dela Gothic One`, `builtin:Barlow Condensed`, `builtin:Chakra Petch`, `builtin:Share Tech Mono`, `builtin:Shippori Mincho B1`, `builtin:DSEG7 Classic`, `builtin:IBM Plex Mono`, `builtin:Major Mono Display`, `builtin:Big Shoulders Display`, `builtin:Courier Prime`, `builtin:Silkscreen`, `builtin:Segoe UI Variable`, `builtin:Georgia`

### Scenes

`base` (the scene of the theme it extends), `none`, `neo-tokyo`, `thermoptic`, `sync-ratio`, `layer-zero`, `bounty`, `skin99`.

<!-- /VALUES -->

## Expert stylesheets

A theme's `extra.css` can restyle anything, with these limits, so a theme can never break
Venuta or reach outside itself:

- Every rule applies only while that theme is in use. Venuta puts each selector under the
  theme: `.rail { … }` becomes the rail of this theme only, and `:root { --r-np: 4px }` sets a
  value for this theme only.
- `url(…)` may only name files in the theme (`url(art/paper.png)`). Anything else is removed.
- `@import`, `@namespace` and the like are removed. Keyframe names are kept to the theme.
- What was removed is listed in the Theme Studio and in the theme's notes in Settings.

## Reliability

- **A broken theme never breaks Venuta.** A value that isn't valid is left out and listed; the
  rest of the theme still works. A theme that can't load at all (not JSON, made for a newer
  Venuta) shows the theme it's built on instead, and Settings says why.
- **Nothing is lost.** Every save keeps the version before it (the Theme Studio's Earlier
  versions). Removing a theme moves it to Set aside.
- **Readable.** The Theme Studio checks text against the page, panels, the rail, menus and the
  player bar (4.5:1), and text on the key colour (3:1), and fixes it in one click.

## Sharing

**Export…** on a theme (its … menu) saves `Name.venuta-theme`, a zip of its folder. **Import…**
or dropping the file on Venuta shows the theme, who made it, what it carries and anything wrong
with it before anything is added. A bare `theme.json` can be imported the same way.

Limits: 25 MB a theme, 64 files, fonts up to 2 MB, pictures up to 12 MB. Themes carry no code.

## Checking a theme by hand

Open Settings, Look and feel: a theme with problems shows a note under its card ("2 notes"),
with each problem and where it is (`tokens.glass`, `fonts.ui`). A theme that can't be used
says why on its card.
