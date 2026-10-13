# Polish pass 2: checklist

Everything asked for on 2026-10-13, in working order. Ticked when built **and** checked in the running app.

## Bugs and consistency
- [x] Now Playing: the title bar, rail and player bar sit on a different background from the panel. Now the window's own backdrop turns to the song's cover and the page fades back (no second background).
- [x] With Now Playing open, the rail did nothing visible (the page changed underneath). Any navigation now closes Now Playing and the queue.
- [x] Hunt for more bugs of this kind (found and fixed: Escape closed every layer at once; the rail did nothing on the open place; Music forgot its tab; waveform missing after a restart; pills flying in from an earlier visit; arrowed scrollbars; the queue opening on old history): overlays that trap you, controls that do nothing visible, states that don't reset (go through every overlay × every way out).

## Mini player
- [x] One always-on-top window, three shapes by size: micro strip, mini bar, card.
- [x] Variants: with cover / without (accent tile), with lyrics / without (no button when a song has none).
- [x] Ways in: player bar button, Ctrl+M, palette "Mini player". Way out: "Back to Venuta".
- [x] Checked in the running app: opens beside a minimised main window, three shapes, lyrics in the card, back to Venuta. (Fixed: the window command deadlocked as a synchronous command.)

## Themes (after research: docs/research/06-themes-and-design.md)
- [x] Theme system: a theme is a full token set (surfaces, rail, bars, text, accents, backdrop treatment, radius, type, motion), not just dark/light.
- [x] Core themes redone to the research: Dark glass, Light glass, Frosted, Solid, AMOLED. Rail, panels, bars, overlays, Now Playing for each.
- [x] Curated accent set that works in dark and light; safe album-derived accents.
- [x] Retro themes: an Akira-inspired one, a Ghost in the Shell-inspired one, plus the best others from the research. Own fonts, backgrounds, borders, progress bars.
- [x] Settings: a theme gallery with live previews.

## Layout and interaction audit
- [x] Buttons too close together or with dead space (Health tiles, EQ presets, Playlists empty state, Organize pills done; keep looking): go through every screen and fix spacing to one scale.
- [x] Album interactions: a card's play button plays, and pauses/resumes when that album is already playing; the same for shelves, the hero, artist pages and song rows. Double-click, Enter and Space behave the same everywhere.
- [x] Every clickable thing has a hover, a pressed and a focus state.

## Added 2026-10-13 (his notes on the player)
- [x] Play icon is not centred in its circle (bar, cards, mini player): centre it optically everywhere.
- [x] Seek bar: the waveform is hard rectangles. Make it Poweramp-like: a smooth, mirrored, rounded waveform with a soft played/unplayed split and a clear playhead.
- [x] Accents: the current fixed swatches don't suit the themes. Replace with the researched set (dark and light variants) and clamp album accents (OKLCH rules).
- [x] Single click plays a song in Up next, the queue, album track lists and song lists (the song playing now: pause/resume). Links inside rows (artist, album) and the ⋯ menu keep their own clicks.
- [x] General review of industry-standard behaviours (hover/press/focus everywhere, consistent click targets, context menus, keyboard).

## Added 2026-10-13 (round 3)
- [x] Settings (and Playlists): scrolling, the side menu slid under the pinned bar and showed through it.
- [x] Accents still clash with the glass theme (an orange accent over a teal-violet backdrop): calmer album accents, follow what's playing rather than what's hovered, and use the accent for fewer things.
- [x] Mini player: retro versions (Neo-Tokyo capsule, Sync Ratio meter, Skin '99 LCD).
- [x] Skin '99's spectrum analyser from real audio (engine computes band levels).
- [x] Layer Zero's red-static shadow, behind the player bar.
- [x] Continue the spacing / button-placement pass on every screen (all places checked at 960x620, 1280 and 1920 wide).
- [x] Every window size: the player bar's right group spilled over the controls; the quality badge needs a better home; set a sensible minimum window size and check each place at it.
- [x] Focus rings: a global radius on :focus-visible squared the round play button.

## Pass 3 (2026-10-13, after playlists)
- [x] Motion "Match Windows" really follows Windows: the attribute CSS reads was "full" unless Reduced was picked, so Windows' reduce-motion setting was ignored; the raw media queries that overrode an explicit "Full" are gone.
- [x] Windows transparency effects off: Glass and Frosted show as Solid (Settings says why); back when they are on.
- [x] Retro motion from the research: Neo-Tokyo pages arrive out of a blur, Thermoptic shimmers, Layer Zero cuts in with a static flash and splits RGB on a song change, Bounty cuts in and wipes its colour blocks across the player bar, Sync Ratio overlays switch on like a monitor. All still under reduced motion.
- [x] Mini player: keep-on-top pin, settles against a screen edge when dropped near one, fades to 85% when left alone, "Up next" in the card; changing shape near the bottom or right edge grows towards the middle and stays on screen.
