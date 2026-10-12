import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource-variable/sora'
import './index.css'
import App from './App.tsx'
import { applyPrefs } from './lib/prefs'

// Before the first paint, so the chosen theme never flashes.
applyPrefs()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
