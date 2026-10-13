# 07: Colour from artwork, and the default theme

Asked 2026-10-13: the default theme (now "Venuta") needs a new name, and its accents don't suit
it. Cider, an Apple Music client with a similar look, is known for better accents: from the track,
or fixed when you want. How does it do it, and what should Venuta do?

## How Cider does it

**Cider 1** (open source, `ciderapp/Cider`, read from the code):
- The accent is **fixed**: `--keyColor` is Apple Music's red `#fc3c44`, or your own colour when
  `visual.customAccentColor` is on (`getAppStyle()` in `src/renderer/main/vueapp.js`). The
  progress bar follows it.
- Colour from the artwork comes from **Apple, not from the pixels**. Every Apple Music artwork
  has a precomputed palette: `bgColor` plus `textColor1` to `textColor4`, chosen to be legible on
  that background. Cider paints album and playlist headers with `bgColor` and puts the title in
  `textColor1`, the subtitle in `textColor2`, details in `textColor3`, and buttons in
  `textColor4` (`views/pages/cider-playlist.ejs`, `playlist-inline.ejs`, `artist.ejs`).
  Everything on a page comes from one coherent set, which is why it looks calm and "right".

**Cider 2** (closed): "Custom UI Accent and Tint Color options", a tweak to let the progress bar
"Use Accent Color", and Immersive backgrounds that are either the artwork blurred and filtered
("Simple Artwork") or "the current track's color scheme as supplied by the Apple Music API"
("Simple Color"). [Cider 2.1](https://cidercollective.itch.io/cider/devlog/564118/cider-v21-release),
[2.3.0](https://v2.cider.sh/changelogs/client-releases/2.3.0).

**Cider 3** (closed): the "Emotion Engine", a "dynamic color adaptation system that extracts
colors from album artwork to theme the interface", with "accent focused UI styling" for menus and
settings. [3.0.0](https://cider.sh/changelogs/3.0.0).

So the pattern is: **an accent you can fix, and a separate, coherent palette from the artwork
(a background plus matching foregrounds) for the surfaces around it.**

## The other reference: Material's content colour

Google's open-source Material Color Utilities pick a cover's colour with a documented scorer
(`score.ts`): colours with chroma below 5 are dropped, and so are hues covering 1% of the image
or less (counted over a 30-degree neighbourhood); each colour scores
`proportion × 70 + (chroma − 48) × (0.1 below 48, 0.3 above)`; picks must differ in hue by as
much as possible (90 degrees down to 15). Then every role (accent, text on accent, containers) is
a **tone of that hue chosen for contrast**, not a fixed lightness.

## What Venuta does now, and why it disappoints

- `accent_color` averages the most saturated seventh of a 32 px copy of the cover. Averaging
  different hues makes mud, and the most saturated pixels are often the **label badges in the
  corners** (the red T-Series or Sony mark on many film covers), not the cover's colour.
- `tuneAccent` then forces every accent to lightness 0.76 to 0.86 and chroma at most 0.13 in
  dark mode, so different albums end up as similar pale pastels on the glass.
- There is one accent and nothing else: no tint for headers, no colour for text on it, no second
  colour. The ten fixed swatches are generic, not designed for this theme.

## Proposal

**Name.** The default theme is the only one without a story; it is glass over your album art.
Candidates: *Lumen* (light through glass; recommended), *Velvet*, *Aurora*, *Halo*, or his
*Elegant*. The app keeps the name Venuta.

**Colour, three modes** (one setting, "Colour"):
1. **From the music** (default): a palette from what's playing, and from the album you open.
2. **One colour**: a curated set designed for this theme, or any colour you pick; Venuta adjusts
   only its lightness so it stays readable, and shows that it did.
3. **Neutral**: no accent; white and graphite, like Apple Music's monochrome controls.

Plus **Intensity**: Subtle, Balanced (default), Vivid, which moves the chroma ceiling.

**A palette, not a pixel average** (computed once per album in the engine, cached with the
thumbnail):
- Quantise the cover in OKLab, weighting the centre over the edges so corner badges count less.
- Score like Material: how much of the cover a hue covers, and how colourful it is; a second
  colour at least 60 degrees away when the cover has one.
- Roles, each solved for contrast rather than clamped:
  - `accent` (buttons, progress, playing): the seed hue at the tone that reaches 3:1 against the
    surface (WCAG 2.2, 1.4.11 for controls);
  - `on-accent`: white or near-black, whichever reads at 4.5:1;
  - `accent-ink` (accent-coloured text): 4.5:1 against the surface;
  - `tint` (album header and Now Playing wash): the seed at low chroma and a dark tone, like
    Apple's `bgColor`, with text colours from the same hue like `textColor1` to `textColor4`;
  - `soft` (selection, pills): the accent at low opacity.
- Greyscale covers keep the fixed colour; changes crossfade, and near-identical colours don't
  trigger a change.

**One accent, one job** (research 06, §1.11): the accent marks the primary action, the playing
state, progress and selection; everything else stays neutral, and the album colour lives in the
tint and backdrop.

**Settings, Look and feel**, in this order: Theme (gallery) → Light, Dark or Match Windows →
Surface → Colour (mode, intensity, swatches or picker) → Album art behind the window. A small
live preview (a button, a progress bar, a playing row) shows the result.

## How it will be judged

A contact sheet of 40 real covers from his library, the current accent beside the new palette
(accent, tint, text), including the label-badge covers, so he can see it before it ships.
