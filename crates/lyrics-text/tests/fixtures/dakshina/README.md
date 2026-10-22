# Dakshina accuracy fixtures

Adapted from the **Dakshina dataset** by Google Research (Roark et al., LREC 2020),
<https://github.com/google-research-datasets/dakshina>, version 1.0, licensed under CC BY-SA 4.0
(see `LICENSE` beside this file); these fixtures are under the same licence.

Each `<lang>.test.tsv` lists the native-script words of the test split's hand-romanized
sentences (`romanized/*.romanized.rejoined.aligned*.tsv`; all 5000 sentences for Hindi, the
first 1000 for the other languages), one per line: the word, how often it occurs, the
romanizations the sentences' annotators gave it (`spelling:count`), and its romanizations in
the lexicons. Made by `cargo run -p lyrics-text --example dakshina -- fixture`.
