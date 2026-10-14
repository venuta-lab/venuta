import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource-variable/sora'
import '@fontsource-variable/figtree'
import '@fontsource-variable/inter'
import '@fontsource-variable/roboto'
// The retro themes' typefaces (OFL, bundled): only downloaded from disk when a theme uses them.
import '@fontsource/dela-gothic-one/400'
import '@fontsource/barlow-condensed/600'
import '@fontsource/barlow-condensed/700'
import '@fontsource/chakra-petch/500'
import '@fontsource/chakra-petch/600'
import '@fontsource/share-tech-mono/400'
import '@fontsource/shippori-mincho-b1/800'
import '@fontsource/dseg7-classic/400'
import '@fontsource/ibm-plex-mono/400'
import '@fontsource/ibm-plex-mono/500'
import '@fontsource/major-mono-display/400'
import '@fontsource/big-shoulders-display/800'
import '@fontsource/big-shoulders-display/900'
import '@fontsource/courier-prime/400'
import '@fontsource/silkscreen/400'
import './index.css'
import App from './App.tsx'
import { MiniPlayer } from './components/player/MiniPlayer'
import { BigScreen } from './components/lyrics/BigScreen'
import { DesktopLyrics } from './components/lyrics/DesktopLyrics'
import { applyPrefs } from './lib/prefs'

// Before the first paint, so the chosen theme never flashes.
applyPrefs()

// The mini player, the big screen and the desktop lyrics are the same app in windows of their
// own (opened at #mini, #stage, #overlay).
const place = location.hash.slice(1)
if (['mini', 'stage', 'overlay'].includes(place)) document.documentElement.dataset.window = place
const Shown = place === 'mini' ? MiniPlayer : place === 'stage' ? BigScreen : place === 'overlay' ? DesktopLyrics : App

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Shown />
  </StrictMode>,
)
