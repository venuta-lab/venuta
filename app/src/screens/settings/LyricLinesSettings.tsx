import { Switch } from '../../components/ui'
import { langBase } from '../../lib/lyricsModel'
import { readerLang, secondChoices, setSecondChoices, systemLang, useSecondChoices } from '../../lib/secondChoice'
import { downloadModel, languageName, removeModel, sizeWords, useTranslationModels } from '../../lib/translation'
import { Group, Row } from './parts'

/*
  Settings > Lyrics, under each line: the two switches' defaults (each song language then keeps
  its own once switched), the language translations are into, and the translation models on this
  PC (downloaded only when asked, removed here).
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
        line="Pronunciation shows a song's words in English letters under each line, for songs in another script. Translation shows what each line means. Both are on every lyrics view (T and Shift+T on the big screen), and each song language keeps its own choice once you switch them."
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
      <TranslationModels into={readerLang(c, system)} />
    </>
  )
}

/** The translation models on this PC: download, size, remove. */
function TranslationModels({ into }: { into: string }) {
  const models = useTranslationModels()
  // Those into the language you read (there are over a hundred pairs in all), and any on this
  // PC or on its way.
  const sorted = models
    .filter((m) => langBase(m.to) === into || m.installed || m.progress !== null)
    .sort((a, b) => Number(langBase(b.to) === into) - Number(langBase(a.to) === into) || a.name.localeCompare(b.name))
  const used = models.filter((m) => m.installed).reduce((n, m) => n + m.size_bytes, 0)
  return (
    <Group
      title="Translation models"
      line="Each translates one language into another, on this PC: nothing is sent anywhere. One is downloaded only when you ask, from its publisher."
    >
      {sorted.length === 0 ? (
        <Row title="None on this PC" hint="Models show here once translation on this PC is available. Translations the lyrics carry are shown already." />
      ) : (
        <>
          {sorted.map((m) => (
            <Row
              key={m.id}
              title={m.name}
              hint={m.progress !== null ? `Downloading, ${Math.round(m.progress * 100)}% of ${sizeWords(m.size_bytes)}` : m.installed ? `${sizeWords(m.size_bytes)} on this PC` : m.error ? `The download failed, try again. ${sizeWords(m.size_bytes)} to download` : `${sizeWords(m.size_bytes)} to download`}
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
    </Group>
  )
}
