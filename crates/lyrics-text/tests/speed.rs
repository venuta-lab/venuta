//! A song's lines are romanized in well under a frame.

use std::time::Instant;

use lyrics_text::{Line, Word, romanize};

const LINES: [&str; 12] = [
    "जो वादे किये सारे बस बातों में रह गये",
    "तुम ही हो, अब तुम ही हो, ज़िंदगी अब तुम ही हो",
    "चैन भी, मेरा दर्द भी, मेरी आशिक़ी अब तुम ही हो",
    "केसरिया तेरा इश्क़ है पिया, रंग जाऊँ जो मैं हाथ लगाऊँ",
    "दिन बीते सारा तेरी फ़िकर में, रैन सारी तेरी ख़ैर मनाऊँ",
    "Baby तू मेरी है, I love you",
    "ਮੈਨੂੰ ਤੇਰੇ ਨਾਲ ਪਿਆਰ ਹੋ ਗਿਆ",
    "நான் உன்னை காதலிக்கிறேன்",
    "আমি তোমাকে ভালোবাসি",
    "నేను నిన్ను ప్రేమిస్తున్నాను",
    "എന്റെ ഖൽബിലെ വെണ്ണിലാവു നീ",
    "ನಿನ್ನ ಕಣ್ಣುಗಳು ತುಂಬಾ ಸುಂದರವಾಗಿವೆ",
];

fn timed(line: &str) -> Vec<Word> {
    line.split_inclusive(' ')
        .enumerate()
        .map(|(i, t)| Word {
            start_ms: i as u64 * 300,
            end_ms: i as u64 * 300 + 300,
            text: t.to_string(),
        })
        .collect()
}

#[test]
fn a_sixty_line_song_in_under_five_ms() {
    let song: Vec<(&str, Vec<Word>)> = (0..60)
        .map(|i| LINES[i % LINES.len()])
        .map(|l| (l, timed(l)))
        .collect();
    // Warm up once (thread-local buffers, first page faults), then take the best of a few runs.
    for (l, w) in &song {
        romanize(&Line::new(l).words(w));
    }
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        for (l, w) in &song {
            std::hint::black_box(romanize(&Line::new(l).words(w)));
        }
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("60 lines: {best:.3} ms");
    assert!(best < 5.0, "60 lines took {best:.3} ms");
}
