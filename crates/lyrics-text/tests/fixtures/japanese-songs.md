# japanese-songs.tsv

65 lines of five Japanese songs, each with the romaji a person wrote for it, for the Japanese
reading gate in `tests/japanese_songs.rs`.

The songs are in the public domain: their words are traditional or their writers died more
than 70 years ago.

| Song | Words | Romaji from |
|---|---|---|
| Furusato (1914) | Takano Tatsuyuki (died 1947) | [English Wikipedia, "Furusato (children's song)"](https://en.wikipedia.org/w/index.php?oldid=1357432402) |
| Sakura Sakura | traditional (Edo period; 1941 school verse) | [English Wikipedia, "Sakura Sakura"](https://en.wikipedia.org/w/index.php?oldid=1379041187) |
| Takeda Lullaby | traditional | [English Wikipedia, "Takeda Lullaby"](https://en.wikipedia.org/w/index.php?oldid=1376669360) |
| Yuki no Shingun (1895) | Nagai Kenshi (died 1940) | [English Wikipedia, "Yuki no Shingun"](https://en.wikipedia.org/w/index.php?oldid=1367082758) |
| Nanatsu no Ko (1921) | Noguchi Ujō (died 1945) | [English Wikipedia, "Nanatsu no Ko"](https://en.wikipedia.org/w/index.php?oldid=1366377378) |

The second column of the file is the Wikipedia revision the line was taken from. The romaji is
as the article writes it, typos included (Nanatsu no Ko's "fuurusu" for 古巣, Yuki no Shingun's
"Ki nomi" for 着の身); the test compares sounds, so spaces, hyphens, capitals, macrons and
"wo" for を don't count.

Licence: the romaji and the selection come from Wikipedia, under the Creative Commons
Attribution-ShareAlike 4.0 licence (https://creativecommons.org/licenses/by-sa/4.0/), credit
to the Wikipedia contributors of the articles above. This file is test data only and is not
shipped in the app.
