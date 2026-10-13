# Polish pass 2: checklist

Everything asked for on 2026-10-13, in working order. Ticked when built **and** checked in the running app.

## Bugs and consistency
- [x] Now Playing: the title bar, rail and player bar sit on a different background from the panel. Now the window's own backdrop turns to the song's cover and the page fades back (no second background).
- [x] With Now Playing open, the rail did nothing visible (the page changed underneath). Any navigation now closes Now Playing and the queue.
- [ ] Hunt for more bugs of this kind: overlays that trap you, controls that do nothing visible, states that don't reset (go through every overlay × every way out).

## Mini player
- [x] One always-on-top window, three shapes by size: micro strip, mini bar, card.
- [x] Variants: with cover / without (accent tile), with lyrics / without (no button when a song has none).
- [x] Ways in: player bar button, Ctrl+M, palette "Mini player". Way out: "Back to Venuta".
- [x] Checked in the running app: opens beside a minimised main window, three shapes, lyrics in the card, back to Venuta. (Fixed: the window command deadlocked as a synchronous command.)

## Themes (after research: docs/research/06-themes-and-design.md)
- [ ] Theme system: a theme is a full token set (surfaces, rail, bars, text, accents, backdrop treatment, radius, type, motion), not just dark/light.
- [ ] Core themes redone to the research: Dark glass, Light glass, Frosted, Solid, AMOLED. Rail, panels, bars, overlays, Now Playing for each.
- [ ] Curated accent set that works in dark and light; safe album-derived accents.
- [ ] Retro themes: an Akira-inspired one, a Ghost in the Shell-inspired one, plus the best others from the research. Own fonts, backgrounds, borders, progress bars.
- [ ] Settings: a theme gallery with live previews.

## Layout and interaction audit
- [ ] Buttons too close together or with dead space: go through every screen and fix spacing to one scale.
- [ ] Album interactions: a card's play button plays, and pauses/resumes when that album is already playing; the same for shelves, the hero, artist pages and song rows. Double-click, Enter and Space behave the same everywhere.
- [ ] Every clickable thing has a hover, a pressed and a focus state.

## Added 2026-10-13 (his notes on the player)
- [x] Play icon is not centred in its circle (bar, cards, mini player): centre it optically everywhere.
- [x] Seek bar: the waveform is hard rectangles. Make it Poweramp-like: a smooth, mirrored, rounded waveform with a soft played/unplayed split and a clear playhead.
- [ ] Accents: the current fixed swatches don't suit the themes. Replace with the researched set (dark and light variants) and clamp album accents (OKLCH rules).
- [x] Single click plays a song in Up next, the queue, album track lists and song lists (the song playing now: pause/resume). Links inside rows (artist, album) and the ⋯ menu keep their own clicks.
- [ ] General review of industry-standard behaviours (hover/press/focus everywhere, consistent click targets, context menus, keyboard).
