import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource-variable/sora'
import './index.css'
import App from './App.tsx'
import { MiniPlayer } from './components/player/MiniPlayer'
import { applyPrefs } from './lib/prefs'

// Before the first paint, so the chosen theme never flashes.
applyPrefs()

// The mini player is the same app in its own small window (opened at #mini).
const mini = location.hash === '#mini'
if (mini) document.documentElement.dataset.window = 'mini'

createRoot(document.getElementById('root')!).render(<StrictMode>{mini ? <MiniPlayer /> : <App />}</StrictMode>)
