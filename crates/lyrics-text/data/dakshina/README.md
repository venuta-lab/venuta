# Common-word tables (from the Dakshina dataset)

These tables are adapted from the **Dakshina dataset** by Google Research
(Brian Roark, Lawrence Wolf-Sonkin, Christo Kirov, Sabrina J. Mielke, Cibu Johny,
Işın Demirşahin and Keith Hall, "Processing South Asian Languages Written in the Latin
Script: the Dakshina Dataset", LREC 2020), <https://github.com/google-research-datasets/dakshina>,
version 1.0. The dataset is licensed under the Creative Commons Attribution-ShareAlike 4.0
International licence (CC BY-SA 4.0), and so are these tables: see `LICENSE` beside this file.

They are data, not code: the rest of the `lyrics-text` crate is under Venuta's MIT licence.

## What was changed

One file per language (`hi`, `mr`, `bn`, `gu`, `pa`, `ta`, `te`, `kn`, `ml`), each line
`native word<TAB>romanization`, sorted by the native word. They were made from the
`lexicons/*.translit.sampled.{train,dev,test}.tsv` files of each language by
`cargo run -p lyrics-text --example dakshina -- table <dataset dir> <languages>`:

- native words are put in NFC, without joiners, with a nasal written one way (see
  `indic::lookup_key`);
- a word is left out when Venuta's rules already spell it the way an annotator did;
- otherwise its spelling is the one nearest Venuta's house style among the spellings at least
  half as common as the commonest one;
- rows whose romanization shares almost nothing with the word (misaligned rows) are left out.
