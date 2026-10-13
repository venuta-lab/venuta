# 06 — Themes and visual design spec

Research date: 2026-10-13. Scope: Venuta's visual system (Tauri 2 / WebView2, React + Tailwind 4, Sora). Covers reference study, five core surfaces, six retro themes, and the mini player.

**How to read the numbers.** Values marked *(src)* come straight from a cited source. Fluent values were read from Microsoft's own theme-resource XAML. Everything else is a design decision made for this spec, not a measurement. No source publishes frame-sampled hex values for the films. The film palettes below are reference picks, and should be checked with an eyedropper against owned Blu-ray stills before shipping.

---

## 1. Reference study

### 1.1 What each product gets right

**Windows 11 Fluent (Mica, Acrylic, layering).** This is the most rigorous public spec and the platform Venuta lives on. These facts are verified:
- **Two layers only.** The *base* layer holds navigation and commands. The *content* layer holds the app's main experience, either as one contiguous surface or as cards ([Layering](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/layering)).
- **Corner radii.** 8px for top-level containers, flyouts and dialogs. 4px for in-page controls *and* for bars (ProgressBar, Slider, ScrollBar). 0px where straight edges meet, and on snapped or maximised windows ([Geometry](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/geometry)).
- **Elevation scale, each with a 1px stroke.** Window and Dialog 128, Flyout 32, Tooltip 16, Card 8, Control 2, Layer 1. A pressed control drops from 2 to 1 (same source).
- **Acrylic recipe.** Background, blur, exclusion blend, tint, then noise. The rules: use background acrylic for transient UI, don't stack acrylic panes edge to edge (it leaves a seam), don't put accent-coloured text on acrylic, and prefer opaque fills for vertical panes that section content. Acrylic falls back to solid when Transparency effects is off, in Battery Saver, or when the window is inactive ([Acrylic](https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic)).
- **Exact tokens** from [`Common_themeresources_any.xaml`](https://github.com/microsoft/microsoft-ui-xaml/blob/main/controls/dev/CommonStyles/Common_themeresources_any.xaml) and [`AcrylicBrush_themeresources.xaml`](https://github.com/microsoft/microsoft-ui-xaml/blob/main/controls/dev/Materials/Acrylic/AcrylicBrush_themeresources.xaml) *(src)*:

| Token | Dark | Light |
|---|---|---|
| SolidBackgroundFillColorBase (Mica fallback) | `#202020` | `#F3F3F3` |
| LayerFillColorDefault (content layer) | `#3A3A3A` @ 30% | `#FFFFFF` @ 50% |
| CardBackgroundFillColorDefault | `#FFFFFF` @ 5% | `#FFFFFF` @ 70% |
| CardStrokeColorDefault | `#000000` @ 10% | `#000000` @ 6% |
| ControlStrokeColorDefault | `#FFFFFF` @ 7% | `#000000` @ 6% |
| TextFillColor Primary / Secondary / Tertiary | `#FFF` / 77% / 53% | `#000` @ 89% / 62% / 45% |
| SmokeFillColorDefault (modal scrim) | `#000` @ 30% | `#000` @ 30% |
| Acrylic default: tint / tintOpacity / luminosity | `#2C2C2C` / 0.15 / 0.96 | `#FCFCFC` / 0 / 0.85 |
| Success / Caution / Critical | `#6CCB5F` / `#FCE100` / `#FF99A4` | `#0F7B0F` / `#9D5D00` / `#C42B1C` |

**Apple Music, macOS 26 Tahoe ("Liquid Glass").**
- Tahoe sidebars float as rounded panels and faintly reflect the wallpaper. Specular highlights move with the window and the cursor.
- For macOS 27, Apple adds a darkened edge and brighter specular highlights for separation, makes sidebars edge-to-edge again, and gives every window the same corner radius ([MacRumors](https://www.macrumors.com/2026/06/09/macos-golden-gate-liquid-glass/)). Read that as Apple partly walking back floating glass sidebars.
- SwiftUI's `containerConcentric` corner configuration derives each inner radius from its container's radius, so radii nest.
- The Now Playing view opens at window level (click the cover or press ⌘⇧F) with syllable-synced lyrics. Critics still found the cover too small ([Hicks](https://hicks.design/journal/apple-music-in-tahoe)). Users reported that the semi-transparent lyrics and playlist panes were hard to read ([Apple Community](https://communities.apple.com/de/thread/256160403)).
- **Lesson:** glass belongs on chrome, not under paragraphs of text.

**Spotify desktop.**
- The 2023 redesign brought desktop in line with the web player and mobile, with a new Library and Now Playing panel ([Spotify newsroom](https://newsroom.spotify.com/tag/desktop)).
- Its structure (observed, not documented): three rounded panels with about 8px radius and 8px gutters, sitting on a black window. Panels are near-black, around `#121212`.
- Album or playlist colour appears as a gradient that bleeds down from the page header and fades into the panel colour over roughly the first 250–330px. It never sits behind the track list.
- Green is reserved for the primary Play button and active states. One accent, one job.

**TIDAL.** Neutral pure-black surfaces, white type, and almost no chrome colour. Colour is saved for quality badges and artwork. A fan recreation describes "a signature neutral black aesthetic" with dynamic Now Playing colours ([Symfonium style](https://styles.symfonium.app/styles/stellar-tidal-music)). Official design write-ups are scarce.

**Plexamp.** **UltraBlur** is a multi-colour gradient sampled from the cover, offered beside a "Smooth" top-to-bottom blend. An optional "Colorize Player Elements" setting tints the seek bar and buttons, and the user can freeze the current colours ([Plex forum guide](https://forums.plex.tv/t/how-to-plexamp-4-6-1-settings/724870)). The seek bar is a waveform. **Lesson:** keep backdrop colour and control colour as separate switches.

**Poweramp (Android) and its skins.** The core app doesn't drive UI colour from album art. Skins rely on album-art blur plus a background *Intensity* control ([forum](https://forum.powerampapp.com/topic/19285-looking-for-a-particular-kind-of-skin/)). Advice in the Liv skin threads is to set Intensity to about 18–30% when bright art makes text unreadable ([forum](https://forum.powerampapp.com/topic/20165-liv-white-poweramp-v3-skin/page/2/)). Dais Dark ships 58 accents and 43 backgrounds ([forum](https://forum.powerampapp.com/topic/22250-dais-dark-poweramp-skin)). **Lesson:** expose a single "backdrop intensity" slider and default it to about 30%.

**Doppler (macOS).** An owned-music player built on restraint. A native vibrancy sidebar holds Library, Presets and Playlists; the album grid is the main view; a hideable Up Next pane sits on the right; transport sits at the bottom ([MacStories](https://www.macstories.net/reviews/doppler-for-mac-offers-an-excellent-album-and-artist-focused-listening-experience-for-your-owned-music-collection/)). This is the closest peer to Venuta.

**foobar2000.** Since 2.0, the frame follows Windows dark mode. The Default UI then needs View › Layout › Quick setup › *Black*, and Columns UI has its own dark mode ([Gigazine](https://gigazine.net/gsc_news/en/20230428-foobar2000-2-major-update), [Columns UI](https://columns-ui-sdk.readthedocs.io/en/latest/dark-mode.html)). **Lesson:** dense list views need very low-contrast row states (3–5% white hover) and must never ship a half-dark state.

**Arc.** The sidebar is "scenery", a frosted surface over one saturated user-chosen gradient. Panes are separated by tinted washes rather than borders. A third-party analysis puts frosted surfaces at about 50–85% white ([open-design](https://open-design.ai/plugins/design-system-arc/)), so treat the number as approximate.

**Linear.** Themes are generated in **LCH** from three inputs: base, accent and contrast. That replaced 98 hand-set variables per theme. The same generator produces elevated surfaces (background, panels, dialogs, modals). Linear also cut the amount of blue in its chrome to look more neutral ([Linear](https://linear.app/now/how-we-redesigned-the-linear-ui)). **Lesson:** generate Venuta's surface ramps from a few seeds in OKLCH; don't hand-tune 40 hexes per theme.

**Raycast.** A blue-tinted near-black, about `#07080a`, with white borders at 4–6% and layered shadows that carry an inset top highlight, which makes cards read as raised glass ([third-party DESIGN.md](https://design.hagicode.com/designs/raycast/DESIGN.md)).

**Nothing OS.** Monochrome, plus a dot-matrix type motif. In NOS 4.0 the motif is used *more intentionally*: Geist is the system face, and dot-matrix appears only where it adds character without hurting readability. Colour now comes from the wallpaper ([Nothing community](https://nothing.community/d/61774-nothing-os-evolution)). **Lesson:** a retro motif is seasoning. Use it on titles, time readouts and empty states, never on the track list.

**Glass in CSS.** Josh Comeau's recipe:
- `blur(16px)` on a backdrop extended to 200% height, so the blur samples content that is about to scroll underneath.
- A 4–6px "edge" strip with `blur(8–12px) brightness(1.1–1.2)` that reads as glass thickness.
- `saturate(120–140%)` to stop blurred colour turning muddy, and a solid fallback inside `@supports`.

Source: [joshwcomeau.com](https://www.joshwcomeau.com/css/backdrop-filter/).

### 1.2 Fifteen transferable rules for Venuta

1. **Two layers, one material each.**
   - Base layer: title bar and rail, sitting on the backdrop.
   - Content layer: the page.
   - Glass is used only for floating chrome (now-playing bar, pinned bar) and transient overlays. Never put glass on glass. *(Fluent, Apple.)*
2. **Text sits on near-opaque surfaces.** Anything holding more than one line of text (lyrics, queue, palette results) gets at least 72% fill opacity in dark mode and at least 80% in light. *(Apple's lyrics complaints.)*
3. **The glass edge is a hairline plus a highlight.**
   - Border: 1px, white at 6–10% (dark) or black at 6–8% (light).
   - Highlight: `inset 0 1px 0` white at 6–10% (dark) or 85–90% (light).
4. **Concentric radii.** Inner radius = outer radius − inset. Use a fixed scale (§2.1).
5. **Shadows follow elevation and nothing else.** Three steps: the pinned bar uses e1, the rail and now-playing bar e2, overlays e3. On AMOLED, replace shadows with strokes.
6. **Saturate whatever you blur.** Album backdrops use `saturate(1.3–1.5)`, glass surfaces `saturate(160–180%)`.
7. **Album colour is ambient, not textual.** It tints the backdrop, the Now Playing view and the accent. It never colours body text, and accent-coloured text never sits on glass. *(Fluent.)*
8. **A scrim and a vignette are mandatory under any art.** Contrast is then guaranteed whatever the cover looks like.
9. **The sidebar differs from content by material, not by heavy borders.** Spotify uses gutters, Arc and Tahoe use tinted washes. For Venuta: the rail is a glass panel, the page is unframed, and an 8px gutter separates them.
10. **Neutrals are slightly tinted.** Push greys about 2–4° toward cool blue (C ≈ 0.005–0.01). Pure `#000` and `#FFF` surfaces look cheap except in AMOLED mode. *(Raycast, Linear.)*
11. **One accent, one job.** The accent marks primary action, current selection, playing state and progress. Everything else is neutral. *(Spotify, TIDAL.)*
12. **Generate ramps; don't hand-pick them.** Every theme is `{base, accent, contrast, radius, font, fx}`, and the surfaces are derived in OKLCH. *(Linear.)*
13. **Honour system fallbacks.**
   - Windows *Transparency effects* off, `prefers-reduced-transparency` or battery saver: switch to the Solid surface.
   - `prefers-reduced-motion`: stop backdrop drift and retro animation.
   *(Fluent.)*
14. **Colour changes crossfade.** Accent and backdrop transition over 600ms using a registered `@property --accent`, with hysteresis so similar covers don't flicker.
15. **Big art.** In the Now Playing view the cover takes at least 55% of the shorter viewport dimension. *(Hicks' critique of Tahoe.)*

---

## 2. Core themes

### 2.1 Shared geometry and recipes (all core surfaces)

- **Radii:**
  - window 8 (Win11; 0 when maximised)
  - rail panel 20
  - now-playing bar 22
  - overlays (palette, queue drawer, dialogs) 16
  - cards and cover tiles 12
  - covers inside lists 6
  - buttons and inputs 10
  - rail active pill 14 (20 − 6 inset)
  - chips and the search pill 999
  - progress track 999 (4px tall, 6px on hover)
- **Gutters:** 8px between the rail, the page and the window edge. The now-playing bar floats 12px above the bottom edge.
- **Glass recipe (CSS):**
  ```css
  .glass{background:var(--surface);backdrop-filter:blur(var(--blur)) saturate(var(--sat));
    border:1px solid var(--border);box-shadow:var(--highlight),var(--shadow);}
  @supports not (backdrop-filter:blur(1px)){.glass{background:var(--surface-solid)}}
  ```
- **Backdrop canvas.** Draw the cover at 64×64 into a canvas, scale it with CSS to cover the window, and apply `filter: blur() saturate() brightness()`. A filter on a tiny canvas is far cheaper in WebView2 than full-window `backdrop-filter`. Drift is a ±4% translate and scale over 40–60s. Keep at most three `backdrop-filter` surfaces on screen at once: rail, pinned bar and now-playing bar. Overlays may add one more while open.
- **Native Mica (optional).** Tauri 2 exposes `windowEffects` with `mica`, `tabbed` and `acrylic`. Mica and Tabbed are Windows 11 only. Acrylic performs badly while the window is dragged or resized, so avoid it ([Tauri window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/)). It needs `transparent: true` and transparent `html`/`body`.

### 2.2 Token tables

Text alpha values are applied to the primary text colour. `e1`, `e2` and `e3` are shadow steps.

**Dark — Glass** (default)

| Token | Value |
|---|---|
| app bg | `#0B0C10` |
| backdrop | `blur(72px) saturate(1.45) brightness(.62)`, opacity .60, scrim `rgba(11,12,16,.30)`, vignette `radial-gradient(120% 90% at 50% 35%, transparent 55%, rgba(0,0,0,.55))` |
| rail | `rgba(20,22,28,.52)`, blur 28, sat 170% |
| panel (cards on page) | `rgba(255,255,255,.045)`, no blur |
| raised / overlay | `rgba(26,28,36,.78)`, blur 40, sat 180% |
| pinned bar | `rgba(11,12,16,.58)`, blur 20, sat 160% |
| now-playing bar | `rgba(22,24,31,.64)`, blur 32, sat 180% |
| border / strong | `rgba(255,255,255,.08)` / `.14` |
| top highlight | `inset 0 1px 0 rgba(255,255,255,.09)` |
| shadow e1 / e2 / e3 | `0 1px 2px rgba(0,0,0,.35)` / `0 8px 24px -6px rgba(0,0,0,.5)` / `0 24px 64px -12px rgba(0,0,0,.65)` |
| text / 2 / 3 / disabled | `#F3F4F7` / 66% / 44% / 28% |
| accent / on-accent | album-derived, else Azure `#5AA8FF` / `#0B0C10` |
| success / warning / danger | `#3DDC97` / `#FFB547` / `#FF6B78` |

**Light — Glass**

| Token | Value |
|---|---|
| app bg | `#EEF0F4` |
| backdrop | `blur(72px) saturate(1.25) brightness(1.08)`, opacity .42, scrim `rgba(244,245,248,.50)`, vignette `radial-gradient(… transparent 60%, rgba(238,240,244,.70))` |
| rail | `rgba(255,255,255,.58)`, blur 28, sat 180% |
| panel | `rgba(255,255,255,.60)` |
| raised / overlay | `rgba(255,255,255,.82)`, blur 40, sat 180% |
| pinned bar | `rgba(246,247,250,.70)`, blur 20 |
| now-playing bar | `rgba(255,255,255,.72)`, blur 32, sat 180% |
| border / strong | `rgba(16,20,32,.08)` / `.14` |
| top highlight | `inset 0 1px 0 rgba(255,255,255,.90)` |
| shadow e1 / e2 / e3 | `0 1px 2px rgba(16,20,40,.06)` / `0 8px 24px -8px rgba(16,20,40,.16)` / `0 24px 60px -12px rgba(16,20,40,.24)` |
| text / 2 / 3 / disabled | `#0F1117` / 62% / 45% / 30% |
| accent / on-accent | album-derived, else Azure `#1F66D1` / `#FFFFFF` |
| success / warning / danger | `#0F8A5F` / `#9D5D00` / `#C42B1C` |

**Frosted ("Mica-like")** is the calm, native-feeling option. The base is near-opaque and only faintly tinted by the cover, the way Mica is tinted by the wallpaper. The rail has **no panel of its own** and sits directly on the base like NavigationView. Content uses Fluent's Layer fill with an 8px top-left radius.

| Token | Dark | Light |
|---|---|---|
| app bg | `#202020` *(src)* | `#F3F3F3` *(src)* |
| backdrop | cover canvas `blur(120px) saturate(1.2)`, opacity .18, under tint `rgba(32,32,32,.80)`; no vignette | same, opacity .14, under tint `rgba(243,243,243,.82)` |
| rail | transparent (pill = `rgba(255,255,255,.06)`) | transparent (pill = `rgba(0,0,0,.04)`) |
| panel (content layer) | `rgba(58,58,58,.30)` *(src)*, stroke `rgba(0,0,0,.10)` | `rgba(255,255,255,.50)` *(src)*, stroke `rgba(0,0,0,.06)` |
| card | `rgba(255,255,255,.05)` *(src)* | `rgba(255,255,255,.70)` *(src)* |
| raised / overlay (acrylic) | `rgba(44,44,44,.86)`, blur 30, sat 125%, 2% noise | `rgba(252,252,252,.88)`, blur 30, sat 125% |
| pinned bar | `rgba(32,32,32,.86)`, blur 20 | `rgba(243,243,243,.86)`, blur 20 |
| now-playing bar | `rgba(44,44,44,.86)`, blur 30 | `rgba(252,252,252,.88)`, blur 30 |
| border | `rgba(255,255,255,.07)` *(src)* | `rgba(0,0,0,.06)` *(src)* |
| top highlight | `inset 0 1px 0 rgba(255,255,255,.06)` | `inset 0 1px 0 rgba(255,255,255,.7)` |
| shadow | e2 `0 8px 16px rgba(0,0,0,.26)`, e3 `0 32px 64px rgba(0,0,0,.37)` | e2 `0 8px 16px rgba(0,0,0,.14)`, e3 `0 32px 64px rgba(0,0,0,.19)` |
| text / 2 / 3 | `#FFFFFF` / 77% / 53% *(src)* | `#000` @ 89% / 62% / 45% *(src)* |
| success / warning / danger | `#6CCB5F` / `#FCE100` / `#FF99A4` *(src)* | `#0F7B0F` / `#9D5D00` / `#C42B1C` *(src)* |

The Frosted shadow values approximate Fluent's Flyout (32) and Dialog (128) elevations.

**Solid** has no translucency anywhere. It is also the automatic fallback for rule 13.

| Token | Dark | Light |
|---|---|---|
| app bg | `#111216` | `#F6F7F9` |
| rail | `#17181D` | `#ECEEF2` |
| panel | `#1B1D23` | `#FFFFFF` |
| raised / overlay | `#22242B` | `#FFFFFF` (+ e3) |
| pinned bar | `#111216` + bottom border | `#F6F7F9` + bottom border |
| now-playing bar | `#1B1D23` | `#FFFFFF` |
| border / strong | `#2A2D35` / `#363A44` | `#DDE0E6` / `#C9CDD5` |
| top highlight | `inset 0 1px 0 rgba(255,255,255,.04)` | none |
| shadow e2 / e3 | `0 8px 24px -8px rgba(0,0,0,.6)` / `0 24px 64px -12px rgba(0,0,0,.7)` | as Light Glass |
| text / 2 / 3 | `#EDEEF2` / `#A3A7B3` / `#6F7380` | `#12141A` / `#5A5F6B` / `#8A8F9B` |
| backdrop | off; Now Playing view only | off; Now Playing view only |

**AMOLED black** (dark only)

| Token | Value |
|---|---|
| app bg | `#000000` |
| backdrop | off by default. Optional "halo": cover canvas only behind the now-playing bar, opacity .25 |
| rail | `#000` + 1px `rgba(255,255,255,.10)` stroke |
| panel | `#0A0A0B` |
| raised / overlay | `#121214` + stroke `.12` |
| pinned bar | `rgba(0,0,0,.82)`, blur 16 |
| now-playing bar | `#0A0A0B` + stroke `.12` |
| border / strong | `rgba(255,255,255,.10)` / `.18` |
| highlight / shadow | none. Elevation is expressed by stroke only |
| text / 2 / 3 | `#FFFFFF` / 70% / 48% |
| accent | dark-mode variants below |

Keep large moving areas off pure black-on-black boundaries. Hover rows use `#0E0E10`, not a translucent white, to limit OLED smear.

### 2.3 Fixed accents (10)

All light variants clear 3.9:1 on white, so they pass the 3:1 non-text rule and most are near 4.5:1. All dark variants clear 6:1 on `#0B0C10`. On-accent text is `#0B0C10` on every dark variant and `#FFFFFF` on every light variant.

| Name | Dark mode | Light mode | OKLCH hue |
|---|---|---|---|
| Ember | `#FF6B4A` | `#D9432A` | 35° |
| Amber | `#FFB547` | `#B86E00` | 70° |
| Lime | `#B5E655` | `#5E8C12` | 125° |
| Mint | `#3DDC97` | `#0F8A5F` | 160° |
| Lagoon | `#3CCFE0` | `#0A7F91` | 205° |
| Azure (default) | `#5AA8FF` | `#1F66D1` | 255° |
| Iris | `#8C8CFF` | `#4B49D6` | 280° |
| Violet | `#C084FC` | `#8B3FD9` | 305° |
| Rose | `#FF6FAE` | `#D02F7A` | 355° |
| Graphite | `#D6D9E0` | `#3A3F4A` | neutral |

Interaction states are derived, not stored. Tailwind 4 is OKLCH-native.

```css
--accent-hover: color-mix(in oklch, var(--accent), white 12%);   /* light theme: black 10% */
--accent-press: color-mix(in oklch, var(--accent), black 12%);
--accent-soft:  color-mix(in oklch, var(--accent) 16%, transparent); /* active rail pill, selection */
```

### 2.4 Album-derived accent rules

1. **Extract.** From the 64×64 thumbnail, run k-means (k = 5) in OKLab. Score each cluster as `population × C`. Candidates need C ≥ 0.06 and population ≥ 4%. If there is no candidate (greyscale or black-and-white covers), use the user's fixed swatch, never a muddy grey.
2. **Clamp in OKLCH**, keeping the hue:
   - Dark themes: L ∈ [0.70, 0.84], C ∈ [0.09, 0.19].
   - Light themes: L ∈ [0.48, 0.60], C ∈ [0.09, 0.20].
   - Yellow and lime hues (85–125°) in light mode: cap L at 0.54, accepting olive-ish yellows.
   - Blue and violet hues (250–300°) in dark mode: floor L at 0.72.
   - Then gamut-map to sRGB by reducing C only.
3. **Contrast gates.**
   - Accent against the page background: at least 3:1 (WCAG 1.4.11).
   - Accent used as text: at least 4.5:1. Otherwise render the label in the text colour plus an accent indicator.
   - On-accent text: choose `#0B0C10` or `#FFF`, whichever contrast is higher. If that is still below 4.5:1, step L by 0.03 until it passes.
4. **The backdrop is not clamped.** It uses the raw cover canvas because the scrim and vignette protect legibility. Only the *accent* is clamped.
5. **Stability.** Change the accent only when ΔE_OK > 0.06, so similar covers in one album keep the same accent. Crossfade over 600ms, cache the result per album, and keep the previous accent while artwork loads.
6. **User control.** Accent source can be *Album*, *Fixed*, or *Album in Now Playing only*. That last option is the TIDAL-like default for the Solid and AMOLED surfaces.

---

## 3. Retro themes

**Shared rules.**
- Each retro theme is a full theme: tokens plus `--font-*`, `--radius-*` and `--fx-*`. It ignores the surface switch, and offers only "effects: full / reduced / off".
- Effects live in one fixed, `pointer-events:none` layer: `#fx::before` and `#fx::after`, plus at most one canvas.
- **Album accent is off by default in every retro theme.** The palette *is* the theme. Album art may still appear, treated to match the theme (duotone, desaturated, framed).
- The track list always keeps a readable body face at 13–14px. Motifs go on titles, time readouts, the now-playing bar and empty states (Nothing rule).

**Chosen themes:** Akira, Ghost in the Shell, plus four that suit a *music* player: Evangelion, Serial Experiments Lain, Cowboy Bebop (jazz) and the Y2K skinned-player era.

**Considered and dropped:**
- Blade Runner: its teal-and-orange neon in rain overlaps Neo-Tokyo and Thermoptic, and amber smoke gives poor UI contrast.
- Tron: a single cyan glow that overlaps Thermoptic.
- 80s synthwave: cliché. A good *stretch* theme, but not hero material.
- Persona 5: its red-black collage overlaps Neo-Tokyo, rotated type hurts list scanning, and the look is closely tied to one publisher's brand.

### 3.1 "Neo-Tokyo"
*Red bike light-trails through a neon night city. Inspired by Akira (1988).*

**Source language.**
- The film used 327 colours, 50 of them mixed specially for its night scenes and five-stage character shading ([Dave Rupert](https://daverupert.com/2022/11/50-colors-akira/), [Seattle Times, 1990](https://archive.seattletimes.com/archive/19900111/1050196/akira-its-animated-but-violent)).
- The red of the bike, jacket and title pushes to the edge of the gamut, against "crushed and dense" blacks ([colorculture](https://colorculture.org/?p=8103)).
- Red and orange light trails convey speed; the city runs cold and blue early in the film ([jgeekstudies](https://jgeekstudies.org/2020/07/19/shadows-of-stained-glass-an-analytical-look-at-animated-horror/)).
- Reference red: `#DA1D1F` ([brandpalettes](https://brandpalettes.com/akira-color-codes/)). The others are picks: night navy `#0A0E1A`, indigo `#141B33`, sodium orange `#FF8A1F`, neon cyan `#35D0E8`, paper cream `#F2EBDD`.

| Token | Value |
|---|---|
| app bg | `#07090F`, plus a hard-stop cel gradient `linear-gradient(180deg,#0A0E1A 0 62%,#07090F 62%)` |
| rail | solid `#0D1120`, 1px right edge `#DA1D1F` @ 60% |
| panel | `rgba(20,27,51,.55)` |
| raised / overlay | `#121830` + 1px `rgba(242,235,221,.12)` |
| pinned bar | `rgba(7,9,15,.90)` + 2px bottom rule `#DA1D1F` |
| now-playing bar | `#0D1120`, 3px left bar `#DA1D1F` |
| border | `rgba(242,235,221,.10)` |
| highlight / glow | red glow `0 0 12px rgba(255,42,42,.45)` on active items only |
| shadow | hard `6px 6px 0 rgba(0,0,0,.6)`, cel style with no blur |
| text / 2 / 3 | `#F2EBDD` / 66% / 42% |
| accent / on-accent | `#DA1D1F` fills (white text, 5:1). Use `#FF4A3D` when red is needed as text |
| accent-2 | `#35D0E8` for links and focus only |
| success / warning / danger | `#3DDC97` / `#FF8A1F` / `#FF4A3D` |

- **Type.** Dela Gothic One for page titles and kanji. It is a very heavy gothic with Latin and Japanese glyphs ([Fontsource](https://fontsource.org/fonts/dela-gothic-one/about), OFL). Anton or Bebas Neue (OFL) for uppercase labels. Sora stays for lists.
- **Background effects.**
  - Generic kanji watermark of original words, e.g. 音楽 ("music") or 再生 ("playback"), never film text. Set vertically in Dela Gothic at 28vh, 5% cream, on the right edge of each page.
  - Film grain at 4%, from an SVG `feTurbulence` data-URI.
  - On track change, horizontal red **light-trail** streaks sweep across: three `linear-gradient(90deg, transparent, #FF2A2A, transparent)` lines, 2px tall, blurred 1px, translating 120vw in 280ms.
- **Shape.** Radius 2. The now-playing bar and overlays get an 8px chamfer on the top-right corner via `clip-path: polygon(...)`. The rail's active item is a red block with cream text, not a pill.
- **Progress.** A 4px red bar. The fill trails a 40px fading gradient behind the playhead (a motion trail), and the playhead is a 14×6 capsule.
- **Motion.** Fast and hard: 120–160ms `cubic-bezier(.2,.9,.1,1)`. Pages slide 24px with a 6px horizontal blur that resolves to sharp.
- **Album accent:** off. Backdrop album art is allowed only as red-black duotone at 20%.

### 3.2 "Thermoptic"
*Rain-damp teal megacity, terminal-green data, and cool steel blue. Inspired by Ghost in the Shell (1995).*

**Source language.**
- The city grade is "cool, saturated, blue": blues and teals with hard bursts of orange and red, and purple highlights. Shadows are thick, highlights roll off, and the whole image is slightly desaturated with a print-film feel ([colorculture](https://colorculture.org/ghost-in-the-shell-cinematography-analysis/)).
- The opening is tinted green with green numbers filling the screen ([TV Tropes, Cyber Green](https://www.tvtropes.org/pmwiki/pmwiki.php/Main/CyberGreen)).
- Hiromasa Ogura's backgrounds are described as muted greys with sickly neon greens ([Halcyon Realms](https://halcyonrealms.com/anime/of-light-darkness-ogura-hiromasa-background-art-book-review)).
- Picks: abyss `#050B0C`, canal teal `#0F2A2B`, city teal `#1E5552`, steel blue `#6E8FA6`, fog `#A9C1C4`, data green `#48F59A`, hard-light orange `#E8743B`, highlight purple `#7A5AA6`.

| Token | Value |
|---|---|
| app bg | `#050B0C` |
| rail | `rgba(10,26,27,.72)`, blur 20, sat 120% |
| panel | `rgba(30,85,82,.12)` |
| raised / overlay | `rgba(8,20,21,.90)`, blur 24 |
| pinned bar | `rgba(5,11,12,.80)`, blur 16 |
| now-playing bar | `rgba(10,26,27,.82)`, blur 24 |
| border | `rgba(72,245,154,.14)` |
| glow | `0 0 0 1px rgba(72,245,154,.18), 0 0 18px rgba(72,245,154,.10)` on focus and active |
| shadow | `0 16px 40px -12px rgba(0,0,0,.7)` |
| text / 2 / 3 | `#D6E6E4` / 64% / 42% |
| accent / on-accent | `#48F59A` / `#03110A` |
| accent-2 | `#6E8FA6` (secondary controls); `#E8743B` only for alerts |
| success / warning / danger | `#48F59A` / `#E8A33B` / `#FF5A4E` |

- **Type.** Chakra Petch (OFL) for titles and labels. Share Tech Mono (OFL) for time, codec and bitrate readouts. Zen Kaku Gothic New (OFL) for Japanese. Sora stays for lists.
- **Background effects.**
  - A faint **data rain**: one canvas with columns of digits and generic katakana at 4% green, falling 8px/s. It turns off under reduced motion.
  - Scanlines: `repeating-linear-gradient(0deg, rgba(0,0,0,.18) 0 1px, transparent 1px 3px)` at 35%.
  - Album art shown as **teal-black duotone**, via an SVG `feColorMatrix` with `#050B0C` → `#48F59A`.
  - The **thermoptic shimmer** plays for 220ms on page change and cover hover: an SVG `feTurbulence` + `feDisplacementMap` (scale 6 → 0) applied to the outgoing view.
- **Shape.**
  - Radius 6 on panels and 3 on controls.
  - Focused and active items get HUD **corner brackets**, drawn as four 8×8 L-corners with background gradients rather than a full border.
  - The rail pill is a 1px green outline with an `rgba(72,245,154,.08)` fill.
- **Progress.** A 2px line with tick marks every 10%. The playhead is a 1×12 caret, and elapsed time reads `02:14.3` in Share Tech Mono. An optional waveform seek bar in green at 40%.
- **Motion.** Calm: 300–450ms ease-in-out. A single 80ms glitch, a 1px RGB split, only on errors.
- **Album accent:** off.

### 3.3 "Sync Ratio"
*Emergency-orange readouts, cockpit purple, and white serif title cards. Inspired by Neon Genesis Evangelion.*

**Source language.**
- The logo colours are orange `#F66E25`, yellow `#F6E201`, black and white ([brandpalettes](https://brandpalettes.com/neon-genesis-evangelion-color-codes/)).
- A Unit-01-style palette: cockpit `#1D1A2F`, purple `#734F9A`, electric green `#8BD450`, warning orange `#E6770B` ([Eggradients](https://www.eggradients.com/palette/evangelion-color-palette); fan palette).
- Motifs: condensed heavy-serif white-on-black title cards, mixing very large and small type; orange line-art monitors; hexagons; and caution stripes.

| Token | Value |
|---|---|
| app bg | `#0B0A10` |
| rail | `#15121F`, 1px right rule `rgba(246,110,37,.35)` |
| panel | `#1D1A2F` |
| raised / overlay | `#241F38` + 1px `#F66E25` @ 50% |
| pinned bar | `#0B0A10` + 1px orange bottom rule |
| now-playing bar | `#15121F` + 1px `#F66E25` @ 40% |
| border | `rgba(246,110,37,.28)` |
| highlight | none (flat monitor look) |
| shadow | `0 0 0 1px #000, 0 12px 30px -10px rgba(0,0,0,.8)` |
| text / 2 / 3 | `#F3EFE6` / 66% / 42% |
| accent / on-accent | `#F66E25` / `#0B0A10` |
| accent-2 / 3 | `#8BD450` (meters, progress) / `#734F9A` (selection fill @ 35%) |
| success / warning / danger | `#8BD450` / `#F6E201` / `#FF2D3A` |

- **Type.**
  - Title cards: Shippori Mincho B1 ExtraBold (OFL), white on black, compressed with `transform: scaleX(.8)` on display text only. The original look uses a commercial mincho, which we don't ship.
  - UI labels: Barlow Condensed SemiBold uppercase (OFL).
  - Time readouts: **DSEG7 Classic** (OFL, keshikan; [Fontsource](https://fontsource.org/fonts/dseg7-classic/about)).
- **Background effects.**
  - A hex-grid SVG pattern at 4% orange.
  - Scanlines at 20%.
  - Caution stripes `repeating-linear-gradient(45deg,#F6E201 0 10px,#0B0A10 10px 20px)`, used *only* on destructive confirms (e.g. delete files).
  - Empty states and page headers become **title cards**: full-bleed black with big and small serif lines.
- **Shape.** Radius 0. The rail active item is an orange left bracket plus a purple fill. The now-playing bar shows volume as a "SYNC %" readout.
- **Progress.** A **segmented** bar with 24 segments and 2px gaps, filled in green. It turns orange in the final 10% of the track, as a countdown.
- **Motion.** Overlays switch on like a monitor, with opacity `steps(3)` over 150ms. Otherwise 160ms linear.
- **Album accent:** off.

### 3.4 "Layer Zero"
*Blue-grey stillness, humming wires, and shadows full of red static. Inspired by Serial Experiments Lain.*

**Source language.**
- Blues, greys and blacks "with only odd splashes of primaries", in a flat and cold mood ([TheaterByte](https://www.theaterbyte.com/serial-experiments-lain-the-complete-collection-blu-ray-review/)).
- Super-high-contrast light and symbolic use of colour ([Okazu](https://okazu.yuricon.com/2013/07/24/serial-experiments-lain-anime-disk-1-english/)).
- Signature motifs: shadows speckled with red, telephone wires against the sky, glowing terminals, and lowercase type.

| Token | Value |
|---|---|
| app bg | `#0A0C10` |
| rail | `#10131A` |
| panel | `#141821` |
| raised / overlay | `#1A1F2A` + 1px `rgba(228,231,236,.10)` |
| pinned bar / now-playing bar | `#0A0C10` 92% / `#10131A` |
| border | `rgba(228,231,236,.09)`; secondary separators 1px dotted |
| shadow | **red-static shadow**: a sibling layer offset 6px down and right, filled with `radial-gradient(#C8193C 1px, transparent 1.4px) 0 0/4px 4px` at 35% |
| text / 2 / 3 | `#E4E7EC` / 58% / 38% |
| accent / on-accent | `#9CC3E6` / `#0A0C10` |
| accent-2 | `#C8193C`, only for the live and now-playing dot and the red static |
| success / warning / danger | `#7FD1AE` / `#E6C26E` / `#E2364E` |

- **Type.**
  - UI: IBM Plex Mono (OFL), lowercase labels.
  - Page titles: Major Mono Display (OFL).
  - Japanese: M PLUS 1 Code (OFL).
- **Background effects.**
  - Faint diagonal "wires" across the top 120px: 3–4 hairlines at 6%, slight catenary curves via SVG.
  - Animated static: SVG turbulence with its seed swapped at 12fps, at 3%.
  - Album art desaturated (`grayscale(.6)`) until hovered.
- **Shape.** Radius 0–2. The rail pill is a solid 2px left rule in pale blue.
- **Progress.** A 1px hairline. The playhead is a block cursor `▌` blinking at 1Hz.
- **Motion.** Stillness, then hard cuts: transitions are 0ms, or a two-frame static flash. On track change, a 1px RGB split for 120ms.
- **Album accent:** off.

### 3.5 "Bounty Session"
*Ink silhouettes on pink, blue, yellow and green blocks, cut to a horn section. Inspired by Cowboy Bebop's opening titles.*

**Source language.** The opening flashes against a black-and-white canvas, then uses pink, blue, yellow and green backdrops under black silhouettes. It pairs them with free-form animated text, with squares and rectangles splitting the frame, all cut to the jazz ([Art of the Title](https://www.artofthetitle.com/title/cowboy-bebop/)). This is the *one light retro theme*: paper and ink.

Picks: ink `#111111`, paper `#EFE6D2`, pink `#E85A8C`, blue `#2F6DB5`, yellow `#F2C230`, green `#3FA66A`, brass `#C08A3E`.

| Token | Value |
|---|---|
| app bg | `#EFE6D2` + 6% paper grain |
| rail | **solid ink** `#111111`, cream labels; active item is a colour block |
| panel | `#F7F0E1` |
| raised / overlay | `#FFF9EC` + 2px ink border |
| pinned bar | the page's colour block, with the title knocked out in ink |
| now-playing bar | `#111111`, cream text, colour-block cover frame |
| border | 2px `#111111` (structural); 1px `rgba(17,17,17,.18)` (rows) |
| shadow | hard offset `4px 4px 0 #111`; pressed `2px 2px 0` + `translate(2px,2px)` |
| text / 2 / 3 | `#111111` / 66% / 46% |
| accent / on-accent | `#2F6DB5` / `#FFFFFF` |
| block rotation | each page has a fixed block colour: Library = blue, Albums = yellow, Artists = pink, Playlists = green |
| success / warning / danger | `#3FA66A` / `#C98A00` / `#D13B2E` |

- **Type.**
  - Display: Big Shoulders Display Black uppercase (OFL).
  - Metadata ("liner notes"): Courier Prime (OFL).
  - Lists: Sora.
- **Background effects.**
  - Paper grain.
  - Halftone dots in block corners: `radial-gradient(#111 1px, transparent 1.3px) 0 0/6px 6px`, masked to a corner.
  - Page headers are full-width colour blocks split into 2–3 rectangles.
- **Shape.** Radius 0; everything is a rectangle with an ink border.
- **Progress.** A 6px ink track filled in the current block colour; the playhead is a 12px square.
- **Motion.** Staccato: elements cut in over 90ms. On track change, colour blocks wipe across in `steps(4)` over 320ms.
- **Album accent:** *quantised*. The album hue snaps to the nearest of the four block colours, which keeps it playful while staying on-palette.
- **Night variant:** ink `#121212` background with paper text.

### 3.6 "Skin '99"
*Brushed gunmetal, bevelled buttons, and a green LCD with a dancing spectrum. Inspired by Y2K-era skinned desktop players.*

**Source language.** Late-90s skinned players: brushed metal, 1px bevels, a black LCD with green segment text, a green-yellow-red spectrum analyser, and a scrolling title marquee. The palette is approximate, drawn from the genre rather than any one skin. Picks: gunmetal `#2B2F3A`, `#3A3F4C`, `#5A6070`; bevel light `#8C93A6`; bevel dark `#14161C`; LCD `#050805`; LCD green `#00E000`; spectrum `#00D000` → `#F0E000` → `#F01000`.

| Token | Value |
|---|---|
| app bg | `#1E2129` |
| rail | `#2B2F3A` + brushed texture + bevel |
| panel | `#262A33` |
| raised / overlay | `#323744` + bevel; title strip `linear-gradient(#4A5060,#2B2F3A)` |
| pinned bar | `#2B2F3A` + bevel |
| now-playing bar | gunmetal body with an inset **LCD** well `#050805` |
| border (bevel) | `inset 1px 1px 0 #8C93A6, inset -1px -1px 0 #14161C`; pressed = inverted |
| shadow | `0 2px 0 #0D0F14` (no soft shadows) |
| text / 2 / 3 | `#DDE2EC` / `#9AA2B4` / `#6B7284` |
| accent / on-accent | `#00E000` / `#031203` |
| success / warning / danger | `#00E000` / `#F0E000` / `#F01000` |

- **Type.**
  - Small-caps labels: Silkscreen (OFL).
  - Time and title marquee: DSEG7 Classic and DSEG14 Classic, with ghost segments `88:88` at 8% behind the real digits.
  - Lists: Sora.
- **Background effects.**
  - Brushed metal: `repeating-linear-gradient(90deg, rgba(255,255,255,.03) 0 1px, transparent 1px 3px)` over a vertical gunmetal gradient.
  - LCD scanlines at 1px.
  - A **live spectrum analyser** (16–20 bars) in the now-playing bar, fed by real FFT data from the audio engine. It is the signature element, and it suits a lossless player.
- **Shape.** Radius 0–2. Buttons are bevelled rectangles. The rail active item is a pressed (inverted-bevel) button with a green LED dot.
- **Progress.** A grooved track (inset bevel) with a 28×10 bevelled knob.
- **Motion.** Effectively none: instant states, with the marquee scrolling at 30px/s.
- **Album accent:** off. The cover sits in a small bevelled frame.

---

## 4. Mini player

### 4.1 What the best ones do

- **Apple Music MiniPlayer (macOS).**
  - The default form is square artwork, with controls appearing on hover.
  - *Hide Artwork* collapses it to a slim strip ([Apple Support](https://support.apple.com/guide/music/mus71d7dcfce/mac)).
  - A lyrics toggle shows lyrics inside the window, and resizing the window scales them.
  - After Tahoe, users reported the art-only state showed no title or artist ([Apple Community](https://communities.apple.com/de/thread/256160403)). **Lesson:** never hide identity entirely.
- **Spotify Miniplayer (2024).**
  - Always on top and freely resizable, from a **thin bar** (art, title, play/pause, skip) through **square** to **rectangle** layouts ([9to5Mac](https://9to5mac.com/2024/03/22/spotify-mac-windows-adds-a-miniplayer/), [Engadget](https://www.engadget.com/spotify-finally-launches-miniplayer-for-desktop-users-171507109.html)).
  - Users complain that art takes about 40% of the height at small sizes and pushes the queue out of view ([community](https://community.spotify.com/t5/Live-Ideas/Desktop-Mini-player-True-mini-player-with-queue-restrict-album/idi-p/7207625)).
  - **Lesson:** cap art size and let extra height go to content.
- **Plexamp.** A compact, phone-proportioned window whose UltraBlur backdrop does the decorating. The waveform seek bar doubles as visual interest.
- **foobar2000.** No built-in mini mode, but users build them from layout panels. The appeal is total control: any panel at any size.
- **Windows media flyout.** The native media card sits above Quick Settings. Third-party FluentFlyout and Flylet replace it with art, title, artist, transport, a seek bar and "Up Next" ([Windows Forum](https://windowsforum.com/news/fluentflyout-native-style-media-flyout-for-windows-11.392322/)). Venuta must publish full SMTC metadata and artwork so these surfaces look right.
- **Dynamic Island and Live Activities.** Size reference ([UX Planet](https://uxplanet.org/unlimited-guide-to-dynamic-island-48700ecc094f), [Infinum](https://infinum.com/?p=28092)):
  - Compact leading and trailing regions: about 52×37pt each, enough for art on one side and a live waveform on the other.
  - Minimal: a 37pt circle.
  - Expanded: about 371 × 84–160pt.
  - Lock-screen Live Activity: at most about 160pt tall.
  - **Lesson:** the smallest state needs just art, a live "playing" signal and one action.

### 4.2 Venuta mini-player variants

The mini player is a separate Tauri window: `decorations:false`, `alwaysOnTop` toggle, `skipTaskbar`, snaps to screen corners, and drops to 85% opacity when idle for more than 3s. It uses the current theme's now-playing-bar tokens and radius. Glass themes get radius 16; retro themes keep their own shapes.

| Variant | Size (px) | Layout | Controls |
|---|---|---|---|
| **Pill** (micro) | 260×44 | Art 32 (r 8) · title–artist marquee · play/pause | Play/pause. Hover shows next and a 2px progress hairline at the bottom edge |
| **Strip, no art** | 300×40 | Animated "playing" bars · title — artist · time | Prev, play/pause, next. 2px progress at the bottom edge. Codec badge on hover |
| **Bar** | 380×64 | Art 48 · title / artist (2 lines) · transport | Prev, play/pause, next, like. Seek bar under the text. Volume on scroll |
| **Card** | 360×132 | Art 100 · title, artist, album · quality badge (e.g. FLAC 24/96) · seek with times | Prev, play/pause, next, shuffle, repeat, like, queue (opens a drawer below) |
| **Square art** | 300×300 (resizable 240–480) | Full-bleed cover. Bottom 40% gradient scrim appears on hover with title, artist, seek and transport. With no hover, a 2px progress line at the bottom edge | Prev, play/pause, next, like. Right-click menu for the rest |
| **Lyrics card** | 360×440 (resizable) | Header row: art 48 + title/artist + play/pause. Body: 5–7 synced lines, current line in text colour at 20px, others at 45%. Backdrop is the blurred cover | Play/pause, prev/next in header. Click a line to seek. "A−/A+" text size |
| **Lyrics, no art** | 360×200 | 3 lines only (previous / **current** / next) on the solid surface. Title in a 12px caption | Play/pause and a hover-only seek |

**Rules.**
- Every size shows title and artist somewhere. Even the art-only square shows them on hover and in its tooltip.
- Art is capped at 50% of the window height in non-square layouts.
- Variants switch automatically by window size, using the thresholds in the table.
- Keyboard shortcuts work while focused: space, arrows, L for lyrics.
- In retro themes the mini player is the hero. Skin '99 becomes an LCD with a spectrum analyser, Sync Ratio a segmented-meter readout, Neo-Tokyo a red capsule pill.
