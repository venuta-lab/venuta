import { useState } from 'react'
import { Switch } from '../../components/ui'
import { langBase } from '../../lib/lyricsModel'
import { readerLang, secondChoices, setSecondChoices, systemLang, useSecondChoices } from '../../lib/secondChoice'
import { downloadModel, forgetKey, languageName, openEnginePage, removeModel, saveKey, sizeWords, testKey, useTranslationKeys, useTranslationModels, type KeyState } from '../../lib/translation'
import { DEFAULT_MODEL, MAKE_TRANSLATIONS, ENGINE_IDS, ENGINES, engineChoice, isKeyed, setEngineChoice, useEngineChoice, type EngineChoice, type EngineId, type KeyedId } from '../../lib/translationEngine'
import { Group, Row } from './parts'

/*
  Settings > Lyrics, under each line: the two switches' defaults (each song language then keeps
  its own once switched), the language translations are into, the engine that translates when
  the lyrics carry none (on this PC, or a service with your own key: the key entered and tried
  here, kept in the system's secret store), and the translation models on this PC (downloaded
  only when asked, removed here).
*/

/** Languages offered for "Translate into", by their tags; English first. */
const INTO = ['en', 'hi', 'es', 'fr', 'de', 'pt', 'it', 'ja', 'ko', 'zh', 'ar', 'ru', 'bn', 'ta', 'te', 'mr', 'pa', 'id', 'tr']

export function LyricLinesSettings() {
  const c = useSecondChoices()
  const set = (change: Partial<typeof c>) => setSecondChoices({ ...secondChoices(), ...change })
  const system = systemLang()
  const systemName = system ? languageName(system) : ''
  const kept = Object.entries(c.byLang).filter(([, v]) => Object.keys(v).length)
  return (
    <>
      <Group
        title="Under each line"
        line={`Pronunciation shows a song's words in English letters under each line, for songs in another script. Translation shows what each line means${MAKE_TRANSLATIONS ? '' : ', when the lyrics come with one in your language'}. Both are on every lyrics view (T and Shift+T on the big screen), and each song language keeps its own choice once you switch them.`}
      >
        <Row title="Pronunciation, at first" hint="For a song language you haven't switched yet. Made on this PC when the lyrics don't carry one.">
          <Switch on={c.defaults.roman} onChange={(on) => set({ defaults: { ...c.defaults, roman: on } })} label="Pronunciation at first" />
        </Row>
        <Row title="Translation, at first" hint="For a song language you haven't switched yet. Only translations in the language below are shown.">
          <Switch on={c.defaults.translation} onChange={(on) => set({ defaults: { ...c.defaults, translation: on } })} label="Translation at first" />
        </Row>
        <Row title="Translate into" hint={`Translations are shown in this language. Now: ${languageName(readerLang(c, system))}.`}>
          <select value={c.into} onChange={(e) => set({ into: e.target.value })} aria-label="Translate into" className="field h-9 rounded-full px-3 text-[13px]">
            {INTO.map((tag) => (
              <option key={tag} value={tag}>
                {languageName(tag)}
              </option>
            ))}
            {systemName && <option value="system">{`The system language (${systemName})`}</option>}
          </select>
        </Row>
        {kept.length > 0 && (
          <Row title="Kept per language" hint={kept.map(([lang]) => (lang === 'und' ? 'Songs of no known language' : `${languageName(lang)} songs`)).join(', ')}>
            <button onClick={() => set({ byLang: {} })} className="btn-ghost btn-sm">
              Back to the defaults
            </button>
          </Row>
        )}
      </Group>
      {MAKE_TRANSLATIONS && <Translation into={readerLang(c, system)} />}
    </>
  )
}

/** Which engine translates: its key, or (on this PC) its models. */
function Translation({ into }: { into: string }) {
  const choice = useEngineChoice()
  const keys = useTranslationKeys()
  const set = (change: Partial<EngineChoice>) => setEngineChoice({ ...engineChoice(), ...change })
  const e = choice.engine
  const info = ENGINES[e]
  const models = useTranslationModels()
  return (
    <Group
      title="Translation"
      line="What translates a song when its lyrics carry no translation in your language. A service with your own key translates poetry, slang and Hinglish better; its lyrics are sent to it only when you translate."
    >
      <Row
        title="Translate with"
        hint={
          e === 'device'
            ? "Offline and free, nothing is sent anywhere. Offered for Spanish and French songs only: in our tests it got other languages' lyrics wrong too often."
            : info.quota
        }
      >
        <select value={e} onChange={(ev) => set({ engine: ev.target.value as EngineId })} aria-label="Translate with" className="field h-9 rounded-full px-3 text-[13px]">
          {ENGINE_IDS.map((id) => (
            <option key={id} value={id}>
              {ENGINES[id].label}
            </option>
          ))}
        </select>
      </Row>
      {info.attribution && (
        // What the service asks of apps that show its translations (Google: say they are
        // automatic translations by Google Translate, mark each line, link to its page, and make
        // its disclaimer available).
        <Row
          title={info.attribution}
          hint={
            <>
              Lines translated by {info.vendor} are automatic translations by Google Translate, marked "{info.attribution}" beside each one. A line you correct by hand is yours and is not marked.
              {info.notice && <span className="mt-1.5 block text-[11px] leading-snug text-ink/45">{info.notice}</span>}
            </>
          }
        >
          {info.page && (
            <button onClick={() => void openEnginePage(e).catch(() => {})} title={info.page} className="btn btn-sm">
              About Cloud Translation
            </button>
          )}
        </Row>
      )}
      {isKeyed(e) && <KeyRows key={e} engine={e} kept={keys.find((k) => k.engine === e)} model={choice.model} into={into} setModel={(model) => set({ model })} />}
      {isKeyed(e) && (
        <Row
          title="Translate automatically with my key"
          hint={`Off: a song is sent to ${info.vendor} only when you press Translate. On: every song you play with Translation on is sent, and uses your quota.`}
        >
          <Switch on={choice.auto} onChange={(on) => set({ auto: on })} label="Translate automatically with my key" />
        </Row>
      )}
      {(e === 'device' || models.some((m) => m.installed || m.progress !== null)) && <ModelRows models={models} into={into} />}
    </Group>
  )
}

/** A key-based engine's key: entered, kept, tried with one short line, forgotten. */
function KeyRows(props: { engine: KeyedId; kept: KeyState | undefined; model: string; into: string; setModel: (m: string) => void }) {
  const { engine: e, kept } = props
  const info = ENGINES[e]
  const [draft, setDraft] = useState('')
  const [region, setRegion] = useState(kept?.region ?? '')
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const failed = (err: unknown) => setNote({ ok: false, text: typeof err === 'string' ? err : 'That didn\'t work.' })
  const save = () => {
    setNote(null)
    void saveKey(e, draft, e === 'microsoft' ? region : null)
      .then(() => {
        setDraft('')
        setNote({ ok: true, text: 'Kept in the system\'s secret store.' })
      })
      .catch(failed)
  }
  const test = () => {
    setBusy(true)
    setNote(null)
    void testKey(e, { key: draft.trim() || undefined, region: e === 'microsoft' ? region : null, model: e === 'llm' ? props.model : undefined, into: props.into })
      .then(([line, back]) => setNote({ ok: true, text: `${info.name} translated "${line}" as "${back}". The key works.` }))
      .catch(failed)
      .finally(() => setBusy(false))
  }
  const where = kept?.kept
    ? `Kept in the system's secret store${kept.plan ? `, a DeepL API ${kept.plan} key` : ''}${kept.region ? `, region ${kept.region}` : ''}. Enter another to replace it.`
    : info.keyHelp
  return (
    <>
      <Row title={`Your ${info.name} key`} hint={where}>
        <span className="flex flex-wrap items-center gap-2">
          <input
            type="password"
            value={draft}
            onChange={(ev) => setDraft(ev.target.value)}
            onKeyDown={(ev) => {
              if (ev.key === 'Enter' && draft.trim()) save()
            }}
            placeholder={kept?.kept ? 'A key is kept' : 'Paste your key'}
            autoComplete="off"
            spellCheck={false}
            aria-label={`Your ${info.name} key`}
            className="field h-9 w-56 rounded-full px-3 text-[13px]"
          />
          <button onClick={save} disabled={!draft.trim()} className="btn btn-sm">
            Save
          </button>
          {kept?.kept && (
            <button onClick={() => void forgetKey(e).then(() => setNote(null)).catch(failed)} className="btn-ghost btn-sm">
              Forget
            </button>
          )}
        </span>
      </Row>
      {e === 'microsoft' && (
        <Row title="Region" hint="Your Translator resource's region, as Azure shows it (westeurope, centralindia). Leave it empty for a global resource. Saved with the key.">
          <input value={region} onChange={(ev) => setRegion(ev.target.value)} placeholder="global" spellCheck={false} aria-label="Region" className="field h-9 w-40 rounded-full px-3 text-[13px]" />
        </Row>
      )}
      {e === 'llm' && (
        <Row title="Model" hint={`Claude's model id. The default is ${DEFAULT_MODEL}.`}>
          <span className="flex items-center gap-2">
            <input value={props.model} onChange={(ev) => props.setModel(ev.target.value)} spellCheck={false} aria-label="Model id" className="field h-9 w-48 rounded-full px-3 text-[13px]" />
            {props.model.trim() !== DEFAULT_MODEL && (
              <button onClick={() => props.setModel(DEFAULT_MODEL)} className="btn-ghost btn-sm">
                Default
              </button>
            )}
          </span>
        </Row>
      )}
      <Row
        title="Test the key"
        hint={note ? <span className={note.ok ? 'text-ink/70' : 'font-medium text-ink/85'}>{note.text}</span> : `Sends one short line to ${info.vendor} with the key you entered, or the one kept.`}
      >
        <button onClick={test} disabled={busy || (!draft.trim() && !kept?.kept)} className="btn-ghost btn-sm">
          {busy ? 'Testing…' : 'Test'}
        </button>
      </Row>
    </>
  )
}

/** The translation models on this PC: download, size, remove. */
/** The models that translate on this PC: one language into another, downloaded only when asked. */
function ModelRows({ models, into }: { models: ReturnType<typeof useTranslationModels>; into: string }) {
  // Those into the language you read (there are over a hundred pairs in all), and any on this
  // PC or on its way.
  const sorted = models
    .filter((m) => (langBase(m.to) === into && m.offered !== false) || m.installed || m.progress !== null)
    .sort((a, b) => Number(langBase(b.to) === into) - Number(langBase(a.to) === into) || a.name.localeCompare(b.name))
  const used = models.filter((m) => m.installed).reduce((n, m) => n + m.size_bytes, 0)
  return (
    <>
      <p className="pt-4 text-[12.5px] leading-relaxed text-ink/55">
        Models on this PC: each translates one language into another, and nothing is sent anywhere. One is downloaded only when you ask, from its publisher.
      </p>
      {sorted.length === 0 ? (
        <Row title="None on this PC" hint="Models show here once translation on this PC is available. Translations the lyrics carry are shown already." />
      ) : (
        <>
          {sorted.map((m) => (
            <Row
              key={m.id}
              title={m.name}
              hint={m.progress !== null ? `Downloading, ${Math.round(m.progress * 100)}% of ${sizeWords(m.size_bytes)}` : m.installed ? `${sizeWords(m.size_bytes)} on this PC${m.offered === false ? ". Not used any more: it got this language's lyrics wrong too often" : ''}` : m.error ? `The download failed, try again. ${sizeWords(m.size_bytes)} to download` : `${sizeWords(m.size_bytes)} to download`}
            >
              {m.progress !== null ? (
                <span className="h-1.5 w-28 overflow-hidden rounded-full bg-ink/[0.12]" aria-hidden>
                  <span className="block h-full rounded-full bg-accent" style={{ width: `${Math.round(m.progress * 100)}%` }} />
                </span>
              ) : m.installed ? (
                <button onClick={() => void removeModel(m.id).catch(() => {})} className="btn-ghost btn-sm">
                  Remove
                </button>
              ) : (
                <button onClick={() => void downloadModel(m.id).catch(() => {})} className="btn btn-sm">
                  Download
                </button>
              )}
            </Row>
          ))}
          {used > 0 && <p className="pt-3 text-[12.5px] text-ink/50">{`${sizeWords(used)} used by translation models.`}</p>}
        </>
      )}
    </>
  )
}
