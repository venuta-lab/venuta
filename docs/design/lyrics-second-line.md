# Lyrics: pronunciation and translation under every line

- **Date:** 2026-10-21
- **Status:** decided, being built
- **Research:** notes 01 (transliteration), 02 (translation) and 03 (Venuta's lyrics code) in the research folder "Lyrics transliteration and translation".

## What the listener gets

Under each lyric line, up to two smaller lines, each with its own switch:

- **Pronunciation**: the same words in English letters, the way Apple Music shows "Jo waade kiye saare bas baaton mein reh gaye" under Hindi. Offered when a song isn't in Latin letters.
- **Translation**: what the line means, in the language you read (English by default, your Windows or Android language if set).

Both switches are on every lyrics view: Now Playing, the big screen, the desktop lyrics and the mini player (and later the Android app). Each is remembered per song language, so Hindi songs can show pronunciation while Spanish songs show translation. Every second line says where it came from ("From the lyrics", "Made on this PC", "Translated on this PC"), and any line can be corrected by hand and is kept.

## Where the second lines come from, best first

### Pronunciation

1. **What the lyrics already carry.** Apple-style lyrics carry romanized lines; NetEase sends romanized lyrics when asked with its newer call; Lyrics+ sends a transliteration per line. When one source has the song in Devanagari and another (Apple's lyrics through Lyrics+) has the same song in English letters, the two are paired line by line by time.
2. **Made by Venuta, offline.** A new Rust crate, `crates/lyrics-text`, shared by Windows and Android:
   - **Indian scripts** (Hindi, Marathi, Nepali, Sanskrit, Bengali, Gujarati, Punjabi, Odia, Tamil, Telugu, Kannada, Malayalam): our own converter written for the casual style people read, not academic spelling. It knows when the short "a" is silent, writes long vowels as aa, ee, oo the way songs are written, handles nasal endings (mein, hain) and keeps a table of common words in their usual spelling, built from Google's hand-romanized Dakshina word lists (shipped as a separate data file under its CC BY-SA 4.0 licence, credited). Target: at least 95% of words spelled the way people spell them, measured on Dakshina's hand-romanized sentences.
   - **Urdu**: only what the lyrics carry, or a Devanagari copy of the song, until a word list is added (short vowels aren't written in Urdu, so rules alone can't do it).
   - **Korean** (rules, exact), **Chinese** (pinyin, phrase-aware), **Russian and other Cyrillic**, **Greek**: libraries under MIT or Apache licences.
   - **Japanese**: needs a dictionary (about 10 MB), downloaded the first time a Japanese song asks for it, never bundled.
   - Thai, Arabic and Hebrew: only what the lyrics carry, for now.
   - Mixed lines are split by script; English words stay as they are.
   - Word timing: the romanized line keeps the original's word timing, so the karaoke highlight runs on both lines.

### Translation

1. **What the lyrics already carry**, with its language: a translation is only shown when it is in the language you read (most translations the Chinese sources carry are Chinese).
2. **Your own corrections**, kept on disk.
3. **On this device, offline, opt-in.** Translation models are downloaded per language only when you switch Translation on for a song in that language (about 20 to 60 MB each), from the model's own publisher, and run on your PC or phone. Nothing is sent anywhere. Which engine (Mozilla's Firefox Translations models, or OPUS-MT on CTranslate2) is chosen by a quality test on 20 real songs in Hindi, Punjabi, Tamil, Spanish, Japanese and Korean, measured offline, before it ships. Models whose licence forbids this use (Meta's NLLB, non-commercial) are never used.
4. **With your own key, opt-in.** For people who want better translations of poetry and slang: DeepL, Microsoft or Google's translation with their own free key, or a language model with their own key. Run only when they ask, never by itself. Venuta never pays for or calls these on its own.

## How it is shown

- One layout rule for every view: the original line, then pronunciation, then translation, smaller and dimmer, wrapped, never clipped. A song keeps the same number of rows for every line, so the desktop strip doesn't change size line by line.
- The big screen fits long lines to the screen with safe margins (shrinks before it clips).
- The desktop lyrics strip grows to fit the rows that are switched on.
- Second lines are worked out once per song in Rust and cached with the lyrics, never per frame.

## Work packages

| WP | What | Depends on | Runs |
|---|---|---|---|
| L1 | The lyrics model and one reader (in progress on `fix/lyrics-sources`): language, per-line romanized and translated text with their source, word timing for your own files, NetEase romanization and translations kept, QQ and Kugou translations, cached answers re-read | none | now |
| L2 | `crates/lyrics-text`: script detection, the Indian-script converter with its common-word table, the Dakshina accuracy test, word-timing mapping | none | now |
| L3 | `crates/lyrics-text`: Korean, Chinese, Cyrillic, Greek, the Japanese dictionary download, mixed lines | L2's crate skeleton | now, beside L2 |
| L4 | Sources: Apple-style TTML romanized and translated lines, Lyrics+ transliteration and translation, NetEase's newer call, pairing a romanized copy with the original | L1 | after L1 |
| L5 | Translation on the device: the 20-song quality test, the chosen engine, model download on demand, a per-song cache | none | now |
| L6 | Every lyrics view: the two switches, the layout rule, the big screen fit, the strip height, the mini player, Settings > Lyrics, corrections by hand, where each line came from | the model contract below | now, against fixtures |
| L7 | Engines with your own key (DeepL, Microsoft, Google, a language model): settings, keys in the system's secret store, explicit runs only | L5's interface | after L5 |

## The contract every package builds against

```text
LyricLine {
  start_ms, end_ms,
  text,                          // the original, as sung
  words: [Word { start_ms, end_ms, text }],
  script,                        // detected: Deva, Guru, Arab, Hira, Hang, Hani, Cyrl, Latn, ...
  roman: Option<Second>,         // pronunciation
  translation: Option<Second>,   // meaning
}
Second {
  text,
  words: [Word],                 // pronunciation only: timed by the original's words
  lang,                          // BCP 47: "hi-Latn" for pronunciation, "en" for translation
  source,                        // Lyrics(provider) | Paired(provider) | Made | Translated(engine) | Corrected
}
Lyrics { lines: [LyricLine], lang: Option<String>, version }
```

## Gates

- Indian-script pronunciation: at least 95% of words as people spell them on Dakshina's hand-romanized Hindi test sentences, and the example line comes out as "Jo waade kiye saare bas baaton mein reh gaye".
- Korean and Chinese: exact on standard test sets; Japanese: readings checked on 50 song lines.
- Translation: chosen engine judged acceptable on the 20-song test (meaning kept on at least 18 of 20 songs), and nothing leaves the device unless the user chose a key-based engine.
- Every view: no clipping or overlap at any size on the big screen, the strip, Now Playing and the mini player, in every theme, with all rows on; lyrics clock within its frame budget.
- Android reuse: `crates/lyrics-text` builds for Android with no platform code.
