import { useCallback, useEffect, useState } from 'react'
import { listRuns, type Run } from '../lib/engine'
import { RunRow } from './ReviewScreen'

/** Everything that changed, newest first, each with undo. */
export function ActivityScreen() {
  const [runs, setRuns] = useState<Run[] | null>(null)
  const load = useCallback(() => {
    listRuns().then(setRuns).catch(() => setRuns([]))
  }, [])
  useEffect(() => {
    load()
  }, [load])
  return (
    <div className="mx-auto max-w-[820px] pb-16 pt-6">
      <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Activity</h1>
      <p className="mt-2.5 max-w-[60ch] text-[14px] text-ink/60">
        Every build of your library, newest first. Undo removes exactly what that build created; your originals are never
        touched.
      </p>
      <div className="mt-6 grid gap-2">
        {runs?.length === 0 && (
          <p className="py-16 text-center text-[14px] text-ink/50">Nothing yet. Builds, edits and answers will appear here.</p>
        )}
        {runs?.map((r) => <RunRow key={r.journal} run={r} onUndone={load} />)}
      </div>
    </div>
  )
}
