import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import { inApp, ytmAccount, type YtmAccountInfo } from './engine'

/** Who is signed in to YouTube Music (null: no one; undefined: still asking), kept current. */
export function useYtmAccount() {
  const [account, setAccount] = useState<YtmAccountInfo | null | undefined>(undefined)
  const reload = useCallback(() => {
    if (!inApp()) return setAccount(null)
    ytmAccount()
      .then(setAccount)
      .catch(() => setAccount(null))
  }, [])
  useEffect(() => {
    reload()
    if (!inApp()) return
    const stop = listen('ytm-account-changed', () => reload())
    return () => {
      void stop.then((f) => f())
    }
  }, [reload])
  return account
}

