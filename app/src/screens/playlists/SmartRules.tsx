import { AnimatePresence, motion } from 'motion/react'
import { useState } from 'react'
import { smartSave, type Condition, type SmartRule } from '../../lib/engine'
import { Segmented } from '../../components/ui'
import { useEscape } from '../../lib/layers'

// `kept`: star ratings aren't given in Venuta any more; a rule that already uses them still
// reads and edits as it was, but new rules don't offer them.
const FIELDS: { id: string; label: string; ops: string[]; hint: string; kept?: true }[] = [
  { id: 'rating', label: 'Rating (stars)', ops: ['at_least', 'at_most', 'is'], hint: '4', kept: true },
  { id: 'unrated', label: 'Not rated yet', ops: [''], hint: '', kept: true },
  { id: 'album_has_rating', label: 'Its album has a song rated', ops: ['at_least'], hint: '4', kept: true },
  { id: 'genre', label: 'Genre', ops: ['contains', 'is'], hint: 'Filmi' },
  { id: 'language', label: 'Language', ops: ['is'], hint: 'hin' },
  { id: 'category', label: 'Category', ops: ['is', 'contains'], hint: 'Film & Series' },
  { id: 'composer', label: 'Composer', ops: ['contains', 'is'], hint: 'A.R. Rahman' },
  { id: 'lyricist', label: 'Lyricist', ops: ['contains', 'is'], hint: 'Gulzar' },
  { id: 'album_artist', label: 'Album artist', ops: ['contains', 'is'], hint: 'Pritam' },
  { id: 'year', label: 'Year', ops: ['at_least', 'at_most', 'is'], hint: '2004' },
  { id: 'added_days', label: 'Added in the last (days)', ops: ['at_most'], hint: '30' },
]
const OP_LABEL: Record<string, string> = { at_least: 'is at least', at_most: 'is at most', is: 'is', contains: 'contains', '': '' }

const stars = (n: string) => `${n} ${n === '1' ? 'star' : 'stars'}`

/** One condition as words that follow "Songs": "rated 4 stars or more", "from 2004 on". */
function phrase(c: Condition): string {
  const v = c.value.trim()
  switch (c.field) {
    case 'rating':
      return c.op === 'is' ? `rated ${stars(v)}` : `rated ${stars(v)} or ${c.op === 'at_least' ? 'more' : 'less'}`
    case 'unrated':
      return 'not rated yet'
    case 'album_has_rating':
      return `from albums with a song rated ${stars(v)} or more`
    case 'year':
      return c.op === 'is' ? `from ${v}` : c.op === 'at_least' ? `from ${v} on` : `from ${v} or before`
    case 'added_days':
      return `added in the last ${v} ${v === '1' ? 'day' : 'days'}`
    default: {
      const label = FIELDS.find((x) => x.id === c.field)?.label.toLowerCase() ?? c.field
      return `whose ${label} ${OP_LABEL[c.op] ?? c.op} “${v}”`
    }
  }
}

/** A rule as a sentence, for the playlist's page: "Songs rated 4 stars or more and from 2004 on." */
export function describeRule(rule: SmartRule): string {
  const parts = rule.conditions.map(phrase)
  const joiner = rule.match === 'all' ? 'and' : 'or'
  const joined = parts.length > 1 ? `${parts.slice(0, -1).join(', ')} ${joiner} ${parts.at(-1)}` : (parts[0] ?? '')
  return `${joined ? `Songs ${joined}` : 'Every song'}${rule.limit ? `, up to ${rule.limit}` : ''}.`
}

/** Edit a smart playlist's rule; saving rewrites the playlist. */
export function SmartRules(props: { root: string; rule: SmartRule; onSaved: () => void; onClose: () => void }) {
  const [rule, setRule] = useState<SmartRule>(props.rule)
  const [saving, setSaving] = useState(false)
  const [problem, setProblem] = useState('')
  const set = (i: number, c: Partial<Condition>) => setRule({ ...rule, conditions: rule.conditions.map((x, j) => (j === i ? { ...x, ...c } : x)) })
  const changed = JSON.stringify(rule) !== JSON.stringify(props.rule)
  useEscape(true, props.onClose)

  async function save() {
    setSaving(true)
    setProblem('')
    try {
      await smartSave(props.root, rule)
      props.onSaved()
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <motion.section
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6 }}
      className="glass mt-8 rounded-[20px] p-5"
      aria-label="Rules"
    >
      <div className="flex flex-wrap items-center gap-3 text-[13.5px]">
        <span className="font-medium">Songs that match</span>
        <Segmented
          label="Match"
          size="sm"
          value={rule.match}
          onChange={(match) => setRule({ ...rule, match })}
          options={[
            { value: 'all', label: 'all of these' },
            { value: 'any', label: 'any of these' },
          ]}
        />
      </div>
      <div className="mt-4 grid gap-2">
        <AnimatePresence initial={false}>
          {rule.conditions.map((c, i) => {
            const f = FIELDS.find((x) => x.id === c.field) ?? FIELDS.find((x) => !x.kept)!
            return (
              <motion.div
                key={i}
                layout
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                className="flex flex-wrap items-center gap-2"
              >
                <select
                  aria-label="What"
                  value={c.field}
                  onChange={(e) => {
                    const nf = FIELDS.find((x) => x.id === e.target.value)!
                    set(i, { field: nf.id, op: nf.ops[0], value: nf.hint })
                  }}
                  className="field min-w-[200px]"
                >
                  {FIELDS.filter((x) => !x.kept || x.id === c.field).map((x) => (
                    <option key={x.id} value={x.id}>
                      {x.label}
                    </option>
                  ))}
                </select>
                {f.ops[0] !== '' && (
                  <>
                    <select aria-label="How" value={c.op} onChange={(e) => set(i, { op: e.target.value })} className="field">
                      {f.ops.map((o) => (
                        <option key={o} value={o}>
                          {OP_LABEL[o]}
                        </option>
                      ))}
                    </select>
                    <input aria-label="Value" value={c.value} onChange={(e) => set(i, { value: e.target.value })} placeholder={f.hint} className="field w-44" />
                  </>
                )}
                <button
                  onClick={() => setRule({ ...rule, conditions: rule.conditions.filter((_, j) => j !== i) })}
                  aria-label="Remove this rule"
                  title="Remove this rule"
                  className="grid h-8 w-8 place-items-center rounded-full text-[17px] leading-none text-ink/45 transition-colors hover:bg-ink/10 hover:text-ink active:scale-90"
                >
                  ×
                </button>
              </motion.div>
            )
          })}
        </AnimatePresence>
      </div>
      <button onClick={() => setRule({ ...rule, conditions: [...rule.conditions, { field: 'added_days', op: 'at_most', value: '30' }] })} className="btn-ghost btn-sm mt-3">
        + Add a rule
      </button>
      <label className="mt-4 flex flex-wrap items-center gap-2 text-[13px] text-ink/75">
        Keep at most
        <input
          type="number"
          min={0}
          value={rule.limit ?? ''}
          onChange={(e) => setRule({ ...rule, limit: e.target.value ? Number(e.target.value) : null })}
          className="field w-24"
          placeholder="any"
        />
        songs
      </label>
      {problem && <p className="mt-3 text-[13px] text-problem">{problem}</p>}
      <div className="mt-5 flex items-center justify-end gap-2">
        <button onClick={props.onClose} className="btn">
          {changed ? 'Cancel' : 'Close'}
        </button>
        <button onClick={() => void save()} disabled={!changed || saving} className="btn-primary">
          {saving ? 'Saving…' : 'Save rules'}
        </button>
      </div>
    </motion.section>
  )
}
