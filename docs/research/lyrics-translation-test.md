# Lyrics translation on the device: the 20-song test

- **Date:** 2026-10-22
- **For:** the plan `docs/design/lyrics-second-line.md`, Translation, layer 3 (work package L5).
- **Question:** which engine translates lyrics on the PC and the phone, offline and free, well enough to show under each line?
- **Answer:** Mozilla's Firefox Translations models (Bergamot), run through `fxtranslate`. They beat OPUS-MT in every language both cover. But neither engine met the plan's gate on this test (meaning kept on 18 of 20 songs):
  - **Spanish:** fine, 4 of 4 songs.
  - **Hindi, Tamil, Japanese and Korean:** at best part of the meaning.
  - **Punjabi:** no usable offline model.
- **Built:** the Bergamot engine (`crates/lyrics-translate`). Whether Translation on this PC ships, and with what wording, is the owner's call (see "What this means").

## Summary

| | Bergamot (Mozilla, `fxtranslate`) | OPUS-MT (Helsinki-NLP, CTranslate2) | IndicTrans2 (AI4Bharat) |
|---|---|---|---|
| Songs whose meaning was kept | **4 of 20** (Spanish 4/4) | **4 of 20** (Spanish 4/4) | not tested |
| Kept or partly kept | 14 of 20 | 10 of 20 | |
| Lost, or no model | 3 lost, 3 with no model (Punjabi) | 10 lost | |
| Made things up | 1 swear word (Korean), 1 word repeated 12 times (Korean) | Bible-society phrases in Hindi and Tamil ("Jehovah", "Watch Tower"), a gun, repeats | |
| Languages covered of the six | 5 (no Punjabi) | 6 (Tamil through a Dravidian group model) | Indian languages only |
| Download per language (gzipped) | 15 MB (Hindi) to 47 MB (Japanese, Korean) | none ready-made; about 80 MB once converted | gated |
| On disk | 23 to 70 MB | 77 to 79 MB (int8) | |
| Memory while translating (Python process, model copied into memory) | +54 MB (Hindi) to +153 MB (Japanese, Korean) | +110 to +113 MB | |
| Load | 0.2 to 0.4 s | 0.5 to 1.2 s | |
| Speed, 1 thread, per line | 55 to 195 ms (portable kernel, no SIMD) | 49 to 170 ms (int8, beam 4) | |
| A 40-line song in Rust, model load included | 1.9 s on 1 thread, 0.77 s on 4, 0.33 s on all 32 | | |
| Licence of the code | MPL-2.0 (`fxtranslate`); its optional SIMD part MIT and BSD-3 | MIT (CTranslate2, `ct2rs`) | MIT |
| Licence of the models | **none stated** (see below); download on request only | Apache-2.0 | MIT, but downloads need an account |
| Rust and Android | pure Rust, nothing to build in C++ | C++ library (CTranslate2) to build for each target | |

The verdict on each song is in "Every song, side by side" below.
- **kept:** a listener reading only the machine line would follow the excerpt.
- **partly:** some lines are right, but the gist is lost or wrong.
- **lost:** wrong or meaningless.

I judged each song against the human translation, reading the original myself. The judging is honest but it is one reader's judgement, and it is generous to both engines in the same way.

## What was tested, and how

### The songs
There are 20 excerpts of 2 to 5 lines in six languages:

| Language | Songs |
|---|---|
| Hindi | 4 |
| Punjabi | 3 |
| Tamil | 3 |
| Spanish | 4 |
| Japanese | 3 |
| Korean | 3 |

Each has a human English translation, and every source is linked from its song below. Every text used is in this file, with both engines' output.

- **Why these songs.** This file is in a public repository, so every excerpt had to be free to copy:
  - **The lyrics** are public domain: traditional, or by lyricists who died long ago.
  - **The translations** are either public domain (Pope 1900 and 1906, Macauliffe 1909, Chamberlain 1880) or written by Wikipedia editors under CC BY-SA 4.0. Those keep their licence and credit here: each links to the exact revision it came from, which lists its authors.
  - **The native text** of every excerpt was copied from the raw page text and checked word for word against it.
- **What that costs the test.** These are older and more poetic texts than what most people play. They include:
  - scripture that is sung (Mool Mantar, Sivapuranam);
  - anthems;
  - 16th to 19th century poems;
  - folk songs in dialect.

  So the test is **harder than modern pop**. Two things are not tested at all:
  - Hinglish and other mixed lines;
  - Hindi written in English letters (which these models cannot read at all).

  Mozilla's own scores on news-style sentences (FLORES-200, from Mozilla's registry) are much better than what this test shows:

  | Model | COMET22 | chrF |
  |---|---|---|
  | hi→en | 0.872 | 62.5 |
  | ta→en | 0.843 | 57.1 |
  | es→en | 0.857 | 58.1 |
  | ja→en | 0.862 | 56.4 |
  | ko→en | 0.864 | 57.5 |

  Song lyrics land between that and this test. Nobody has published a test on lyrics.
- **How each line went in.** One line per sentence, as Venuta sends them: no song context, each song as one batch.

### The engines
- **Bergamot.** These are Mozilla's Firefox Translations models: the newest release per pair, from Mozilla's model registry.
  - **Hindi** uses the `tiny` model (3.0).
  - **Tamil and Spanish** use `base-memory` (3.0).
  - **Japanese and Korean** use the desktop `base` model (3.0). Phones get `base-memory` (3.1), which the registry scores the same, at 0.862 and 0.864 COMET22.

  Each ran with its lexical shortlist, through `fxtranslate` 0.5.0 (the PyPI build of the same Rust engine, then the Rust crate itself). Both builds ran the portable kernel: the published Windows build has no SIMD, and Venuta's build leaves it off on purpose (see Android below). The Rust crate gave the same lines as the Python build.
- **OPUS-MT.** These are Helsinki-NLP's models, Apache-2.0, from Hugging Face:
  - `opus-mt-hi-en`
  - `opus-mt-pa-en`
  - `opus-mt-dra-en` (Dravidian languages, for Tamil, since there is no `ta-en`)
  - `opus-mt-es-en`
  - `opus-mt-ja-en`
  - `opus-mt-ko-en`

  Each was converted to CTranslate2 int8 (`ct2-transformers-converter`, CTranslate2 4.8.3) and run with beam 4 on 1 thread. CTranslate2 is the C++ library `ct2rs` binds, so the speed is what `ct2rs` would get.
- **IndicTrans2** (AI4Bharat). The models are MIT and cover all 22 scheduled Indian languages, Punjabi included. **Not tested.** Every official download needs an account:
  - The Hugging Face models (`ai4bharat/indictrans2-indic-en-dist-200M` and the rest) are gated.
  - The fairseq and CTranslate2 checkpoints sit in the gated `ai4bharat/BPCC` dataset.

  The rules for this work allow no services that need an account. The same rule would make Venuta's "download when asked" impossible, because each listener would need a Hugging Face login. Unofficial ungated copies exist but were not used. If AI4Bharat ever publishes an ungated download, it is the best candidate for Punjabi (see "What is left").
- **The machine.** Windows 11, 32 logical cores. Times are wall clock, after one warm-up line. Memory is the process's resident growth from start to the largest seen.

## Licences and the terms of each download source

| What | Licence | Where it comes from, and its terms |
|---|---|---|
| `fxtranslate` 0.5.0 (the engine Venuta uses) | MPL-2.0. File-level copyleft: Venuta stays MIT, and fxtranslate's own files stay MPL and are used unchanged from crates.io | crates.io. Vendored inside it: gemmology (MIT) and xsimd (BSD-3), used only by the `simd` feature, which is off |
| Mozilla's model files | **No licence is stated** on the files, in the registry JSON, or in Firefox's Remote Settings records. The archived repository that used to ship them (`mozilla/firefox-translations-models`) was MPL-2.0 as a whole. Firefox downloads them to every user | Downloaded from the translations project's public bucket (`storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data`), the address the model registry gives. No terms are attached to it. `fxtranslate`'s README asks products not to lean on Firefox's own Remote Settings CDN ("provisioned for Firefox, not for third-party traffic"), so Venuta does not use that CDN; it reads Remote Settings once, when the list is made, only to pin the checksums. **So: downloaded only when the listener clicks Download, never bundled.** Confirm the licence with Mozilla before ever shipping a model inside the app |
| OPUS-MT models | Apache-2.0 (the `tc-big` ones are CC BY 4.0) | Hugging Face, no account needed for these. A model would have to be converted to CTranslate2 and hosted by someone, since there are no official CTranslate2 copies |
| CTranslate2, `ct2rs` | MIT | crates.io and PyPI |
| IndicTrans2 | MIT (models and code) | Hugging Face, **gated** (account and accepting terms) |
| NLLB-200 | CC BY-NC 4.0: not usable | not tested |

## What this means

1. **Use Bergamot.** It is better than OPUS-MT wherever both have a model:
   - **Hindi:** 2 songs partly kept against 0. OPUS-MT invented Bible-society sentences.
   - **Tamil:** 2 against 0. Again OPUS-MT invented sentences.
   - **Japanese and Korean:** both partly kept; Bergamot's lines were closer.
   - **Spanish:** a tie.

   It is also lighter:
   - 15 to 47 MB downloads against about 80 MB;
   - half the memory for Hindi;
   - pure Rust, with no C++ library to build for Windows and two Android targets.

   It is fast enough: a 40-line song takes 1.9 s on one core with the model load, and 0.33 s on all cores. Venuta uses half the cores, up to 8.
2. **The gate is not met.** This decides whether Translation on this PC ships:
   - **Spanish** (and, from Mozilla's scores, the other European languages) is good enough.
   - **Hindi, Tamil, Japanese and Korean lyrics** come out half right at best on this test. That is a test harder than pop, but these are the languages Venuta's listeners asked about.

   Options for the owner:
   - (a) Ship it as is, with the line labelled as a machine translation, which the views already do ("Translated on this PC").
   - (b) Ship it only for the pairs that pass. That needs a second test on modern songs, with a bilingual reader for each language, which can't be published here.
   - (c) Keep it off by default and point to the key-based engines of L7 (DeepL, Microsoft, Google, a language model) for poetry.

   My recommendation is (a). It is opt-in per song with the size shown, it costs nothing, nothing leaves the PC, and the label is honest. Also do (b)'s spot check on the owner's own library before calling it done.
3. **Punjabi has no offline answer.**
   - Mozilla has no Punjabi model.
   - OPUS-MT's pa-en produced nonsense on all three songs.
   - IndicTrans2 needs an account to download.

   For Punjabi songs, Translation on this PC is not offered: no model is listed. The L7 engines cover Punjabi (DeepL, Microsoft).
4. **Hindi in English letters** (common in lyrics files) can't be translated offline: the model reads Devanagari only. Venuta leaves such lines untranslated, and every line not in the song's own script is skipped, so English lines in a Hindi song are left alone too.
5. **Two kinds of failure to guard against:**
   - **A word repeated in a loop.** Venuta cuts it back to as many times as the original sings it (`lines::tidy`).
   - **Invented content.** Nothing can catch it offline. It is why the line must say it is a machine translation, and why a listener can correct any line by hand (L6 keeps corrections).

## Android
- **The engine builds for Android with no C++ library.** `fxtranslate` is pure Rust plus an optional C++ SIMD kernel, which Venuta leaves off. The rest of the crate (`ureq` with rustls, `flate2`, `sha2`, `serde`) is pure Rust as well.
- **Why the SIMD kernel stays off.** It is compiled for one CPU level with no check at run time:
  - on aarch64, `-march=armv8.4-a+i8mm`, which crashes on phones without i8mm (most before 2022);
  - on x86-64, AVX2.

  A build that knows its CPU can turn it on with the `simd` feature. It does not compile under MSVC anyway: the Windows build falls back to the portable kernel.
- **The model list picks the phone's models** on Android (`Platform::current()`): `base-memory` for Japanese, Korean and Chinese, 44 MB instead of 60 MB unpacked.
- **What the Android app must supply:**
  - the app data folder (Venuta's `paths::app_data_dir` on Android);
  - a call to `Translations::download` only from a button;
  - nothing else.

  **Checked:** `cargo ndk -t arm64-v8a -t x86_64 build -p lyrics-translate --release` builds with NDK 29 (2026-10-22). The only C in it is `ring`, the TLS library under `ureq`, which every other networked crate in Venuta pulls in too. It has not been run on a phone.
- **Speed on a phone is not measured.** The portable kernel on a phone's big core is perhaps 3 to 5 times slower than this PC's single thread (an estimate). That would put a 40-line song at a few seconds on four cores. It runs once per song and is then kept.
- **CTranslate2, for comparison,** would need its C++ library cross-built for each Android ABI, with a BLAS backend (Ruy). That is one more reason against it.

## Every song, side by side

### Hindi

#### hi-01: Saare Jahan Se Achha (Tarana-e-Hind)

- **Words:** Muhammad Iqbal (1877-1938), 1904. Original: [source](https://en.wikipedia.org/w/index.php?title=Sare_Jahan_se_Accha&oldid=1381007386).
- **Human English:** English Wikipedia editors (original translation, CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Sare_Jahan_se_Accha&oldid=1381007386).
- **Meaning kept:** Bergamot **lost** (bulbul read as bubbles, ghurbat (exile) lost); OPUS-MT **lost** (hallucinated Jehovah in line 4).
- **Corpus note:** First two couplets. The Devanagari is the article's 'Text in the Devanagari script' section (an Urdu poem written in Devanagari, i.e. Hindustani). Line-aligned 4:4. Stripped `<br/>` and the `[[Hindustan]]` link. Sung as a patriotic song in India since the 1930s (article: 'sung during morning assemblies, and as a marching song').

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| सारे जहाँ से अच्छा हिन्दोसिताँ हमारा | Better than the entire world, is our Hindustan, | All where the best of ours | The best leftovers from all over ours |
| हम बुलबुलें हैं इसकी यह गुलसिताँ हमारा | We are its nightingales, and it (is) our garden abode. | We are the bubbles of this gullition of ours | We're the bubbles thereof, and it's ours. |
| ग़ुर्बत में हों अगर हम, रहता है दिल वतन में | If we are in an alien place, the heart remains in the homeland, | If we're in the heart, we're in the heart | If we live in sanctikat, living in the heart |
| समझो वहीं हमें भी दिल हो जहाँ हमारा | consider us too [to be] right there where our heart would be. | Understand that we have a heart where we | How can we make sure that our heart is complete toward Jehovah? |

#### hi-02: Payo Ji Maine Ram Ratan Dhan Payo

- **Words:** Mirabai (16th century). Original: [source](https://en.wikipedia.org/w/index.php?title=Payo_Ji_Maine_Ram_Ratan_Dhan_Payo&oldid=1312266012).
- **Human English:** English Wikipedia editor(s) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Payo_Ji_Maine_Ram_Ratan_Dhan_Payo&oldid=1312266012).
- **Meaning kept:** Bergamot **partly** (names left in Roman letters; lines 3 and 4 half right); OPUS-MT **lost** (invented names).
- **Corpus note:** First 4 of 6 lines. The language is Rajasthani/Braj-flavoured Hindi in Devanagari, not modern standard Hindi. The page's English is one prose paragraph per couplet; I split its first paragraph at the sentence break to get 4:4 alignment (no words changed). The double space before ॥ in line 2 is as on the page. The English has grammar slips ('it gets increases') kept as published. Popular bhajan, sung by D. V. Paluskar and Lata Mangeshkar (article).

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| पायो जी मैंने राम रतन धन पायो । | I have attained the treasure of God's name. | Payo ji I got Ram Ratan Dhan. | Payo G. I found Ramo Röllio. |
| वस्तु अमोलिक दी मेरे सतगुरु, किरपा करी अपनायो  ॥ | My Guru God, gifted me with this priceless gift, and I have accepted this with great gratitude. | The object was Amolik Di My Satguru, Kirpa Kari Apna. | The object gave me the amicate, even my master, Karpadi, his voice. |
| जनम जनम की पूँजी पाई, जगमें सभी खोवायो । | I found the most desired treasure of my life, but have lost the worldly humdrum possessions of the world. | The capital of birth, all the lost in the world. | Janham's savings were found, all in the world lost. |
| खरचै न खूटै चोर न लूटै दिन दिन बढ़त सवायो ॥ | This treasure of Lord's name, neither gets extinguished, nor gets stolen by thieves. Actually, it gets increases multiple times day by day. | Kharchai na Khutai thief does not rob, day by day, the rise. | The hypothetical thief has not been robbed on the third day of the day |

#### hi-03: Babul Mora Naihar Chhooto Hi Jaye

- **Words:** Wajid Ali Shah (1822-1887), thumri written c. 1856. Original: [source](https://en.wikipedia.org/w/index.php?title=Babul_Mora_Naihar_Chhooto_Hi_Jaaye&oldid=1376790353).
- **Human English:** English Wikipedia editors (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Babul_Mora_Naihar_Chhooto_Hi_Jaaye&oldid=1376790353).
- **Meaning kept:** Bergamot **lost** (babul (father) read as Babylon, piya as drinking); OPUS-MT **lost** (repetition loop in line 4).
- **Corpus note:** Hindustani (Awadhi-flavoured) thumri; famously sung by K. L. Saigal in the film Street Singer (1938). Line-aligned 5:5. The repeated first line was dropped, and the refrain cue ' | बाबुल मोरा ...' (wikitext '`{{!}}` बाबुल मोरा ...') at the end of lines 3 and 5 was removed. '(उठायें)' is an alternative reading given on the page; the English 'lift' follows it. Stripped the `[[palanquin]]` link.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| बाबुल मोरा, नैहर छूटो ही जाए | O My father! I'm leaving home. | Babal Mora, Naihar leave | Babylon's Mora, Vestuja |
| चार कहार मिल, मोरी डोलिया सजावें (उठायें) | The four (palanquin) bearers lift my palanquin. | Four haar mills, Mori dolias decorated (range) | Four petitioner found, Mori Petrea (transfering) |
| मोरा अपना बेगाना छूटो जाए | I'm leaving those who were my own. | Mora should leave his own | Let Mora leave your step |
| आँगना तो पर्बत भयो और देहरी भयी बिदेश | Your courtyard is now like a mountain, and the threshold, a foreign country. | Angana is the servoic, fear and the dehri feared | And the Court of the Gentiles, the Court of the Gentiles, and the Court of the Gentiles. |
| जाए बाबुल घर आपनो मैं चली पीया के देश | I leave your house, father, I am going to my beloved's country. | Go to Babylon, I went to the land of drinking | Go to Babylon home, you know I go and drink |

#### hi-04: Banjaranama

- **Words:** Nazeer Akbarabadi (1735-1830). Original: [source](https://en.wikipedia.org/w/index.php?title=Banjaranama&oldid=1378236346).
- **Human English:** English Wikipedia editor(s) (CC BY-SA 4.0); see why_free for the refrain caveat. [Source](https://en.wikipedia.org/w/index.php?title=Banjaranama&oldid=1378236346).
- **Meaning kept:** Bergamot **partly** (lines 1 and 4 right, the moral lost); OPUS-MT **lost**.
- **Corpus note:** An Urdu/Hindustani nazm; the Devanagari is the page's 'Devnagri transcription' column. Line-aligned 5:5. Stripped `<br>`. Weakest 'sung' case in the set: it is mostly recited, though it is also sung (the page does not document performances).

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| गर तू है लक्खी बंजारा और खेप भी तेरी भारी है | If you are a millionaire, and your stores are brimming, | If you are the lakhi Banjara and the consignment is also your heavy | If you are a fool, you are also heavy and heavy. |
| ऐ ग़ाफ़िल तुझसे भी चढ़ता इक और बड़ा ब्योपारी है | Know, O ignorant! There is always another merchant who is even greater than you. | O Ghafil is a great one and a great man. | You are one of the heedless. |
| क्या शक्कर मिसरी क़ंद गरी, क्या सांभर मीठा-खारी है | What of your sugar, candy, jaggery and nuts? What of your doughs, sweet and salty? | What sugar misri kand gari, is sambhar sweet-khari | Is the sugar of Egypt sweet and sweet?- |
| क्या दाख मुनक़्क़ा सोंठ मिरच, क्या केसर लौंग सुपारी है | What of your grapes, raisins, ginger and pepper? What of your saffron, cloves and betel? | What a wine dry ginger, whether saffron cloves are betel nut | Whether the grapes are cascincap, what's Kushing betel nut? |
| सब ठाठ पड़ा रह जावेगा जब लाद चलेगा बंजारा | All your splendor will lie useless, when the nomad packs-up and leaves. | Everything will be stunned when the servy will run | It's all the fun we've got when we're loaded |

### Punjabi

#### pa-01: Mool Mantar

- **Words:** Guru Nanak (1469-1539); opening of the Guru Granth Sahib (compiled 1604). Original: [source](https://en.wikipedia.org/w/index.php?title=Mul_Mantar&oldid=1357450495).
- **Human English:** English Wikipedia editors (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Mul_Mantar&oldid=1357450495).
- **Meaning kept:** Bergamot **none** (no Punjabi model); OPUS-MT **lost** (only karta purakh (creator man) right).
- **Corpus note:** First 5 of 6 lines (the sixth is ਗੁਰ ਪ੍ਰਸਾਦਿ॥ 'known by the Guru's grace'). Taken from the second table on the page (Gurmukhi / Transliteration / Translation); the first table's translations are quoted from published scholars and were not used. Language is Sant Bhasha (old Punjabi) in Gurmukhi. Sung in kirtan. Stripped `<br>`. Very short, terse phrases.

| Original | Human | OPUS-MT |
|---|---|---|
| ੴ ਸਤਿ ਨਾਮੁ | One creator, name is truth, | Don't hide |
| ਕਰਤਾ ਪੁਰਖੁ | agentive (doer) being, | Creator Man |
| ਨਿਰਭਉ ਨਿਰਵੈਰੁ | without fear, without hatred, | Bealoga |
| ਅਕਾਲ ਮੂਰਤਿ | timeless form, | Cal image |
| ਅਜੂਨੀ ਸੈਭੰ | unbegotten, self-existent, | Oral |

#### pa-02: Deh Shiva Bar Mohe Ehai

- **Words:** Guru Gobind Singh (1666-1708), from Chandi Charitar (Dasam Granth). Original: [source](https://en.wikipedia.org/w/index.php?title=Deh_Shiva_Bar_Mohe_Eha&oldid=1378241313).
- **Human English:** English Wikipedia editors (presumed; unsourced) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Deh_Shiva_Bar_Mohe_Eha&oldid=1378241313).
- **Meaning kept:** Bergamot **none** (no Punjabi model); OPUS-MT **lost** (invented words).
- **Corpus note:** The whole hymn. The language is Braj Bhasha written in Gurmukhi, not modern Punjabi. The page gives 8 English lines for 4 Gurmukhi lines; I joined them in pairs with a space to get 4:4 (no words changed). The page's line 2 holds two verse lines split by `<br/>`; split there. Widely sung (e.g. in the film Nanak Naam Jahaz Hai, 1969).

| Original | Human | OPUS-MT |
|---|---|---|
| ਦੇਹ ਸਿਵਾ ਬਰੁ ਮੋਹਿ ਇਹੈ ਸੁਭ ਕਰਮਨ ਤੇ ਕਬਹੂੰ ਨ ਟਰੋਂ ॥ | O Siva, (Epithet for the almighty) give me this boon, May I never ever shirk from doing good deeds, | Recabands and Talsabbband |
| ਨ ਡਰੋਂ ਅਰਿ ਸੋ ਜਬ ਜਾਇ ਲਰੋਂ ਨਿਸਚੈ ਕਰਿ ਅਪੁਨੀ ਜੀਤ ਕਰੋਂ ॥ | that I shall not fear when I go into combat. And with determination I will be victorious. | Stalle Sokbobi |
| ਅਰੁ ਸਿਖ ਹੋਂ ਆਪਨੇ ਹੀ ਮਨ ਕੌ ਇਹ ਲਾਲਚ ਹਉ ਗੁਨ ਤਉ ਉਚਰੋਂ ॥ | That I may teach myself this creed alone, to speak only of Thy (almighty) praises. | And if you want to, you need to know how much you love him. |
| ਜਬ ਆਵ ਕੀ ਅਉਧ ਨਿਦਾਨ ਬਨੈ ਅਤਿ ਹੀ ਰਨ ਮੈ ਤਬ ਜੂਝ ਮਰੋਂ ॥ | And when the last days of my life come, I may die in the might of the battlefield. | Avibsting Gori will take place at least once |

#### pa-03: Gagan Mai Thaal (Aarti), stanza 3

- **Words:** Guru Nanak (1469-1539), Guru Granth Sahib, Rag Dhanasari, p. 663. Original: [source](https://en.wikipedia.org/w/index.php?title=Gagan_mein_thaal&oldid=1377604334).
- **Human English:** Max Arthur Macauliffe, 1909 (public domain). [Source](https://en.wikisource.org/w/index.php?title=The_Sikh_Religion/Volume_1/The_Sohila&oldid=7767635).
- **Meaning kept:** Bergamot **none** (no Punjabi model); OPUS-MT **lost** (line 2 a made-up sentence).
- **Corpus note:** Third stanza of the Sikh Aarti, sung daily as part of Kirtan Sohila and in kirtan. Line-aligned 4:4. Gurmukhi taken from the Wikipedia page with the Roman transliteration after `<br />` removed. Language is Sant Bhasha (old Punjabi/Braj mix). Macauliffe italicised 'real'; the italics were dropped. Archaic English ('becometh', 'pleaseth').

| Original | Human | OPUS-MT |
|---|---|---|
| ਸਭ ਮਹਿ ਜੋਤਿ ਜੋਤਿ ਹੈ ਸੋਇ ॥ | The light which is in everything is Thine, O Lord of light. | All lighty light is light |
| ਤਿਸ ਕੈ ਚਾਨਣਿ ਸਭ ਮਹਿ ਚਾਨਣੁ ਹੋਇ ॥ | From its brilliancy everything is brilliant; | As a result, I was moved to tears because I knew that I would be able to see her again. |
| ਗੁਰ ਸਾਖੀ ਜੋਤਿ ਪਰਗਟੁ ਹੋਇ ॥ | By the Guru's teaching the light becometh manifest. | Adec bearing Light |
| ਜੋ ਤਿਸੁ ਭਾਵੈ ਸੁ ਆਰਤੀ ਹੋਇ ॥੩॥ | What pleaseth Thee is the real worship. | Those Who Are Working Hard |

### Tamil

#### ta-01: Yaadhum Oore Yaavarum Kelir (Purananuru 192)

- **Words:** Kaniyan Pungundranar, Sangam period (c. 1st-3rd century CE). Original: [source](https://en.wikipedia.org/w/index.php?title=Purananuru&oldid=1378876710).
- **Human English:** George Uglow Pope, 1906 (public domain). [Source](https://en.wikipedia.org/w/index.php?title=Purananuru&oldid=1378876710).
- **Meaning kept:** Bergamot **lost** (the negation in line 2 dropped, kelir (kin) as sir); OPUS-MT **lost**.
- **Corpus note:** First 3 of 13 lines. Pope's rhymed verse has 14 lines and drifts from the Tamil lineation after line 3, so only the cleanly matching opening was kept. Classical Sangam Tamil, much older than modern Tamil. Set to music as the theme song of the 10th World Tamil Conference (Kaniyan Pungundranar article). Stripped `<br />` and trailing tabs.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| யாதும் ஊரே; யாவரும் கேளிர்; | To us all towns are one, all men our kin, | It's all a single; everyone's a sir; | and all that lived in the land, |
| தீதும் நன்றும் பிறர் தர வாரா; | Life's good comes not from others' gifts, nor ill, | evil and good to give to others; | Sun and others on Sunday; |
| நோதலும் தணிதலும் அவற்றோரன்ன; | Man's pains and pain's relief are from within, | The disease and the quenchiness are among them; | and are in a state of confusion, |

#### ta-02: Sivapuranam (Tiruvasagam, hymn 1), opening

- **Words:** Manikkavasagar (9th century). Original: [source](https://ta.wikisource.org/w/index.php?title=%E0%AE%A4%E0%AE%BF%E0%AE%B0%E0%AF%81%E0%AE%B5%E0%AE%BE%E0%AE%9A%E0%AE%95%E0%AE%AE%E0%AF%8D/%E0%AE%9A%E0%AE%BF%E0%AE%B5_%E0%AE%AA%E0%AF%81%E0%AE%B0%E0%AE%BE%E0%AE%A3%E0%AE%AE%E0%AF%8D&oldid=1277551).
- **Human English:** George Uglow Pope, 1900 (public domain). [Source](https://archive.org/details/tiruvacagamorsac00maniuoft/page/1).
- **Meaning kept:** Bergamot **partly** (vazhga as long live kept, thal (foot) as sheet); OPUS-MT **lost** (hallucinated Watch Tower sentence).
- **Corpus note:** Lines 1-5, line-aligned 5:5. Tamil from Tamil Wikisource (trailing spaces and the line-number template `{{Pline|5|r}}` removed). Pope's own printed Tamil differs slightly (e.g. 'வாஅழ்க', 'அனேகன்'); the Wikisource text was used. English typed from the page scan: Pope prints thin spaces before '!' (normalised away) and transliterates கோகழி as 'Gōgar̤i' (r with two dots below, U+0324). Devotional hymn sung daily in Shaiva worship; archaic, liturgical register.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| நமச்சிவாய வாழ்க நாதன் தாள் வாழ்க | Hail, the five letters! Hail, foot of the Lord! | Namachivaya Long live Nathan Sheet | However, it is worth a lot of time and money. |
| இமைப்பொழுதும் என் நெஞ்சில் நீங்காதான் தாள் வாழ்க | Hail, foot of Him Who not for an instant quits my heart! | You're always alive in my heart | Sooner and later in my mind, I will be a queen forever |
| கோகழி ஆண்ட குருமணிதன் தாள் வாழ்க | Hail, foot of the Guru-pearl that rules in Gōgar̤i! | Long live the Gurumanithan Sheet of Kozhi | The Watch Tower Society’s Watch Tower Publications Index reports that “more than three - and - a - half - year - olds have been able to locate a single - year - old girl. ” |
| ஆகமம் ஆகிநின்று அண்ணிப்பான் தாள் வாழ்க | Hail, foot of Him Who becomes, abides, draws near as the Āgamam! | Long live the sister-in-law's sheet | Umbrellan Paper Lives on |
| ஏகன் அநேகன் இறைவன் அடிவாழ்க | Hail, foot of Him, the One, the Not-One, and the King! | Agan is the Lord of many | The one that has survived is the one that has proved to be so great that it has never been found in human history. |

#### ta-03: Tiruppalliyezhuchi (Tiruvasagam, hymn 20), stanza 1

- **Words:** Manikkavasagar (9th century). Original: [source](https://ta.wikisource.org/w/index.php?title=%E0%AE%A4%E0%AE%BF%E0%AE%B0%E0%AF%81%E0%AE%B5%E0%AE%BE%E0%AE%9A%E0%AE%95%E0%AE%AE%E0%AF%8D/%E0%AE%A4%E0%AE%BF%E0%AE%B0%E0%AF%81%E0%AE%AA%E0%AF%8D%E0%AE%AA%E0%AE%B3%E0%AF%8D%E0%AE%B3%E0%AE%BF%E0%AE%AF%E0%AF%86%E0%AE%B4%E0%AF%81%E0%AE%9A%E0%AF%8D%E0%AE%9A%E0%AE%BF&oldid=458413).
- **Human English:** George Uglow Pope, 1900 (public domain). [Source](https://archive.org/details/tiruvacagamorsac00maniuoft/page/207).
- **Meaning kept:** Bergamot **partly** (praise, Lord Shiva and the place kept); OPUS-MT **lost** (unrelated quote in line 3).
- **Corpus note:** The 'awakening' hymn sung at dawn (especially in the month of Margazhi). Each Tamil long line matches two printed lines in Pope, which I joined with a space to get 4:4. Small drift: 'Çiva-Lord' (சிவபெருமானே) ends Pope's line 2 but belongs to Tamil line 3. Footnote numbers after 'Perun-turrai' and 'Bull' were removed; the small capitals of the refrain are written as capitals. Tamil kept as on Wikisource, including the spaces before '!' and ';' and the double space in 'எனை  உடையாய்'. Archaic English.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| போற்றி ! என்வாழ் முதல் ஆகிய பொருளே! புலர்ந்தது ; பூங்கழற் கிணைதுணை மலர்கொண்டு | Hail! Being, Source to me of all life’s joys! ’Tis dawn; upon Thy flower-like feet twin wreaths of blooms we lay, | Praise! The first thing in my life! The flower of the flower, with the help of the sage | Hey! My first thing! |
| ஏற்றிநின் திருமுகத்து எமக்கருள் மலரும் எழில்நகை கொண்டுநின் திருவடி தொழுகோம்! | And worship, ’neath the beauteous smile of grace benign that from Thy sacred face beams on us. Çiva-Lord, | Let's pray with the scenic sage of the hoisting face! | Let's pray for the symbiotic stylishing! |
| சேற்றிதழ்க் கமலங்கண் மலரும்தண் வயல்சூழ் திருப்பெருந் துறையுறை சிவபெருமானே! | Who dwell’st in Perun-turrai girt with cool rice-fields, where ’mid the fertile soil th’ expanding lotus blooms! | Suddhuthazhu Kamalangan blossoms and thangal fields, Thiruperundurai, Lord Shiva! | “ We’ve had a very good time there, ” she says, “but it’s very difficult to get out of it. |
| ஏற்றுயர் கொடியுடையாய்! எனை  உடையாய்! எம்பெருமான் பள்ளி எழுந்தருளாயே ! | Thou on Whose lifted banner is the Bull! Master! Our mighty Lord! FROM OFF THY COUCH IN GRACE ARISE! | You're the flag of the loader! You're my roar! Emperuman's school is up! | Look at me! |

### Spanish

#### es-01: La Cucaracha (refrain)

- **Words:** Traditional; this refrain from the Mexican Revolution era (1910s). Original: [source](https://en.wikipedia.org/w/index.php?title=La_Cucaracha&oldid=1376650501).
- **Human English:** English Wikipedia editors (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=La_Cucaracha&oldid=1376650501).
- **Meaning kept:** Bergamot **kept** (line 4 says than smoking); OPUS-MT **kept**.
- **Corpus note:** Line-aligned 4:4, a literal rather than singable translation. Plain, modern Spanish.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| La cucaracha, la cucaracha, | The cockroach, the cockroach, | The cockroach, the cockroach, | The cockroach, the cockroach, |
| ya no puede caminar | can't walk any more | Can't walk anymore | He can't walk anymore. |
| porque no tiene, porque le falta | because it doesn't have, because it's lacking | because he doesn't have, because he's missing | because he has no, because he lacks |
| marihuana que fumar. | marihuana to smoke. | marijuana than smoking. | marijuana to smoke. |

#### es-02: La Bayamesa (Cuban national anthem)

- **Words:** Perucho Figueredo (1818-1870), 1868. Original: [source](https://en.wikipedia.org/w/index.php?title=La_Bayamesa&oldid=1377644960).
- **Human English:** English Wikipedia editors (presumed; unsourced) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=La_Bayamesa&oldid=1377644960).
- **Meaning kept:** Bergamot **kept**; OPUS-MT **kept** (bayameses as berries).
- **Corpus note:** First 4 lines, aligned 4:4. ''' bold markup stripped. 19th-century register ('corred', 'temáis' in the vosotros form).

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| ¡Al combate, corred, bayameses!, | To combat, run, people of Bayamo! | To the battle, run, Bayamese! | Into combat, run, berries! |
| Que la patria os contempla orgullosa; | Because the Fatherland proudly looks at you; | That the fatherland beholds you proud; | That the fatherland beholds you proud; |
| No temáis una muerte gloriosa, | Fear not a glorious death, | Do not fear a glorious death, | Fear not a glorious death, |
| Que morir por la patria es vivir. | Because dying for the Fatherland is living. | That to die for the homeland is to live. | That dying for the homeland is living. |

#### es-03: Himno Nacional Argentino (opening)

- **Words:** Vicente López y Planes (1785-1856), 1813. Original: [source](https://en.wikipedia.org/w/index.php?title=Argentine_National_Anthem&oldid=1375553500).
- **Human English:** English Wikipedia editors (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Argentine_National_Anthem&oldid=1375553500).
- **Meaning kept:** Bergamot **kept**; OPUS-MT **kept**.
- **Corpus note:** First 4 lines, aligned 4:4. The `[[Argentine Declaration of Independence|...]]` link was stripped from the English line 3. Early-19th-century register ('Oíd', 'ved').

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| Oíd, mortales, el grito sagrado: | Harken, mortals, the sacred cry: | Hear, mortal, the sacred cry: | Hear, mortals, the sacred cry: |
| "¡Libertad! ¡Libertad! ¡Libertad!" | "Freedom! Freedom! Freedom!" | "Freedom! Freedom! Freedom!" | "Freedom! Freedom! Freedom!" |
| Oíd el ruido de rotas cadenas | Hear the sound of broken chains | Hear the noise of broken chains | Hear the noise of broken chains |
| ved en trono a la noble igualdad | See noble equality enthroned. | He sees on the throne the noble equality | See the throne of noble equality |

#### es-04: Ríu Ríu Chíu (villancico), stanza on the wolf

- **Words:** Anonymous (sometimes attributed to Mateo Flecha), printed in the Cancionero de Upsala, 1556. Original: [source](https://en.wikipedia.org/w/index.php?title=R%C3%ADu_R%C3%ADu_Ch%C3%ADu&oldid=1329075448).
- **Human English:** English Wikipedia editors (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=R%C3%ADu_R%C3%ADu_Ch%C3%ADu&oldid=1329075448).
- **Meaning kept:** Bergamot **kept** (line 3 subject wrong); OPUS-MT **kept** (line 3 thin).
- **Corpus note:** The page gives the English as one quoted prose sentence; I split it at its comma and semicolons to align 4:4 and dropped the surrounding quotation marks and the `[[original sin]]` / `[[Virgin Mary|virgin]]` links (no words changed). Renaissance Spanish (e.g. 'Quísola', double negative 'Ni aun ... no'). A much-recorded Christmas carol.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| El lobo rabioso la quiso morder | The raging wolf sought to bite her, | The rabid wolf wanted to bite her | The rabid wolf wanted to bite her. |
| Mas Dios Poderoso la supo defender | but God Almighty knew (how) to defend her; | But God the Power of God knew how to defend her | But God the Mighty could defend her. |
| Quísola hacer que no pudiese pecar | He chose to make her so that she could not sin; | I want to make him not be able to sin | To make him not to sin. |
| Ni aun original esta virgen no tuviera. | no original sin was found in that virgin. | Not even original this virgin did not have. | Even this virgin didn't have an original. |

### Japanese

#### ja-01: Sakura Sakura (1888 verse)

- **Words:** Traditional; this verse as printed in the Tokyo Academy of Music's koto collection, 1888. Original: [source](https://en.wikipedia.org/w/index.php?title=Sakura_Sakura&oldid=1379041187).
- **Human English:** English Wikipedia editors (presumed; unsourced) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Sakura_Sakura&oldid=1379041187).
- **Meaning kept:** Bergamot **partly** (sky, as far as the eye can see and cloud kept; mist and fragrance lost); OPUS-MT **partly** (weaker than Bergamot).
- **Corpus note:** First 5 of 7 lines of the original (1888) verse; the 1941 Ministry of Education verse was not used. Line-aligned 5:5. The ideographic spaces (U+3000) are as on the page. 弥生 is literally the third lunar month; the English renders it as 'spring'. Classical grammar (e.g. 'ぞ ... 出ずる').

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| 桜　桜 | Cherry blossoms, cherry blossoms, | Sakura Sakura | Sakura. |
| 弥生の空は | Across the spring sky, | Yayoi's sky | It's the sky. |
| 見渡す限り | As far as the eye can see. | As far as the eye can see. | As far as the eye can see, |
| 霞か雲か | Is it mist, or clouds? | A cloud or a cloud. | It's a cloud or something. |
| 匂いぞ　出ずる | Fragrant in the air. | It smells. It's out. | I smell it. I'm getting it. |

#### ja-02: Hotaru no Hikari (verse 1)

- **Words:** Lyrics by Chikai Inagaki (died 1913), 1881, set to the tune of Auld Lang Syne. Original: [source](https://en.wikipedia.org/w/index.php?title=Hotaru_no_Hikari&oldid=1364383929).
- **Human English:** English Wikipedia editor(s) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Hotaru_no_Hikari&oldid=1364383929).
- **Meaning kept:** Bergamot **partly** (light, snow and parting kept; fireflies lost); OPUS-MT **partly** (light, snow and leaving kept).
- **Corpus note:** Line-aligned 4:4. Old orthography as printed (kyūjitai 螢, 窗, 讀 and the iteration mark ゝ). Meiji-era literary Japanese; 'すぎ' is a pun (passing years / cedar door).

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| 螢の光、窗の雪、 | Light of fireflies, (moonlight reflected off) snow by the window. | The light of the sardon, the snow of the sardon, | The light of the fire, the snow of the hail. |
| 書讀む月日、重ねつゝ。 | Many days and months spent reading. | A month of writing, slapping. | I'm going to write it on Monday, and I'm going to write it. |
| 何時しか年も、すぎの戸を、 | Before one knows it, years have passed. The door | A year, too old, too-to-door, | It's always the age. It's the past door. |
| 開けてぞ今朝は、別れ行く。 | we resolutely open; this morning, we part ways. | Open it up, I’m going to break up this morning. | Open up this morning, we're leaving. |

#### ja-03: Kimigayo

- **Words:** Anonymous waka from the Kokin Wakashu (905); anthem melody 1880. Original: [source](https://en.wikipedia.org/w/index.php?title=Kimigayo&oldid=1380325801).
- **Human English:** Basil Hall Chamberlain, 1880 (public domain). [Source](https://en.wikipedia.org/w/index.php?title=Kimigayo&oldid=1380325801).
- **Meaning kept:** Bergamot **partly** (until the moss grows kept; names left in Roman letters); OPUS-MT **partly** (thousand, eight thousand, moss kept).
- **Corpus note:** NOT line-aligned: 5 Japanese lines and 4 English lines; Chamberlain's version is a free, rhymed, Victorian rendering. Score at the passage level, not line by line. The `[[Emperor of Japan|my lord]]` link was stripped. Kanji column used (the page also has a hiragana column).

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| 君が代は | Thousands of years of happy reign be thine; | you're the generation. | You didn't. |
| 千代に八千代に | Rule on, my lord, until what are pebbles now | Chiyo to Yachiyo | It's a thousand-year-old. Eight thousand. |
| さざれ石の | By ages united to mighty rocks shall grow | sazare stone | A rosestone. |
| 巌となりて | Whose venerable sides the moss doth line. | Be a sl taki. | I'll be in the middle of something. |
| 苔の生すまで |  | Until the moss grows. | Until the moss grows. |

### Korean

#### ko-01: Doraji taryeong (verse 1)

- **Words:** Traditional Korean folk song (minyo). Original: [source](https://en.wikipedia.org/w/index.php?title=Doraji_taryeong&oldid=1364388100).
- **Human English:** English Wikipedia editor(s) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Doraji_taryeong&oldid=1364388100).
- **Meaning kept:** Bergamot **partly** (line 3 right; line 1 repeats Doraji 12 times); OPUS-MT **partly** (lines 1 and 3 right).
- **Corpus note:** Line-aligned 4:4. `{{lang}}` / `{{Transliteration}}` templates and `<br />` stripped. The English keeps 'doraji' (bellflower root) untranslated. The English adds '!' that the Korean lacks. The translation is loose: '반실만 되누나' means the basket is (half) filled.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| 도라지 도라지 도라지 | Doraji, doraji, doraji! | Doraji Doraji Doraji Doraji Doraji Doraji Doraji Doraji Doraji Doraji Doraji Doraji | Dorazi Dorazi Dorazi. |
| 심심산천의 백도라지 | In the depths of the mountains is white doraji! | Baekdo-raji of Simsimsancheon | A centrifuge of Simic acid. |
| 한두 뿌리만 캐어도 | Though one or two roots only I pull, | Just a couple of roots. | One or two roots. |
| 대바구니로 반실만 되누나 | my bamboo basket grows full. | I'm only half-roomed with a big-bassil. | It's a basket. |

#### ko-02: Sae taryeong (Bird song), second section

- **Words:** Traditional Jeolla minyo, late Joseon (19th century); attributed to Kim Sam-jin. Original: [source](https://en.wikipedia.org/w/index.php?title=Sae_Taryeong&oldid=1364389751).
- **Human English:** English Wikipedia editor(s) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Sae_Taryeong&oldid=1364389751).
- **Meaning kept:** Bergamot **partly** (lines 1, 2 and 5 right); OPUS-MT **partly** (lines 2 and 5 right, a flying cat).
- **Corpus note:** First 5 lines of the second section, line-aligned 5:5. The typo 'Logevity' is as published. Dense Sino-Korean phrases (산고곡심 무인처 울림비조, 쌍거쌍래) and dialect spelling (왼갖, 잘허는); the translation is free in places.

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| 새가 날아든다 왼갖 잡새가 날아든다 | Birds are flying, all sorts of birds are flying | The birds fly, the left-wing birds fly. | There's a bird flying, and there's a cat flying left. |
| 새 중에는 봉황새, 만수문전의 풍년새 | The phoenix of all birds, in front of the Logevity Gate, bearer of good harvests | Among the birds is the phoenix bird, the phoenix of Mansumun. | One of the birds is the bong stork, the ferret bird of the Mansom temple. |
| 산고곡심 무인처 울림비조 뭇새들이 | In a deep valley with high mountains: many birds fly in the forest on this sunny spring day | Sangok Heart Unmanned Ullimbijo | I've seen all the wild, colorful, impregnable birds. |
| 농춘화답에 짝을 지어 쌍거쌍래 날아든다 | Birds fly in pairs, singing as though chatting about the pleasant spring weather | They're paired with a pair of farm-and-up-fired-down-threts-tup-thm-tup-th-up | He's a pair of wildflowers and he's flying like a couple. |
| 말 잘허는 앵무새, 춤 잘 추는 학 두루미 | Parrots, good at chatting; cranes, good at dancing | A well-dancing parrot, a well-dancing crane | A talking parrot, a dancing crane. |

#### ko-03: Monggeumpo taryeong

- **Words:** Traditional Hwanghae minyo, late Joseon (19th century); attributed to Kim In-sook. Original: [source](https://en.wikipedia.org/w/index.php?title=Monggeumpo_Taryeong&oldid=1364389237).
- **Human English:** English Wikipedia editor(s) (CC BY-SA 4.0). [Source](https://en.wikipedia.org/w/index.php?title=Monggeumpo_Taryeong&oldid=1364389237).
- **Meaning kept:** Bergamot **partly** (lines 3 and 5 right; line 4 invents a swear word); OPUS-MT **partly** (lines 3 and 5 right; line 4 invents a gun).
- **Corpus note:** First verse with refrain, then verses 2-4; line-aligned 5:5. Italic '' markup stripped from the refrain in both languages. Hanja glosses in parentheses (今日, 上峯, 行船, 城隍) are as on the page. Dialect and spelling variants (멀구요, 외로은).

| Original | Human | Bergamot | OPUS-MT |
|---|---|---|---|
| 장산곶 마루에 북소리 나더니 금일(今日)도 상봉(上峯)에 님 만나 보겠네 | At the peak of Jangsangot mountain a drum beats; I will meet my love there today. | I will see you on the floor of the Cape of Changsan, and I will see you on the day of the meeting. | I heard a drum sound on the top of the hill and I'll see you on the golden eve. |
| (후렴) 에헤이요 에헤이요 에헤이야 님 만나 보겠네 | (refrain) E-he-i-yo e-he-i-yo e-he-i-ya, I will meet my love. | Eheyo Eheyo Eheyo Eheiya | I'm going to meet you. |
| 갈 길은 멀구요 행선(行船)은 더디니 늦바람 불라고 성황(城隍)님 조른다 | I have a long way to go, the ship is moving slow; I pray again and again to the earth god to blow winds to move the ship. | There is a long way to go, and the journey is slow, so the fire of the wind is slow, and the fire is slow. | It's a long way to go, and the line is slow, and the wind is slow and the wind is blowing. |
| 님도 보구요 놀기도 하구요 몽금이 개암포 들렸다 가게나 | I will see my love, and play along the way; I will stop by at the Mong-geum-yi gae-am-po (Harbor). | I'm playing with you, too. I'm playing. I'm having a dream come a shit. | I see you, I play with you. I can't believe you've got a gun on your head. |
| 바다에 흰돛 쌍쌍이 도으나 외로은 사랑엔 눈물만 겨워라 | White sails turn in the sea; the lonely lover’s heart is filled with tears. | There are two white sails in the sea, but only tears in love alone. | There are pairs of white sails in the sea, but only tears in the love of the lonely. |
