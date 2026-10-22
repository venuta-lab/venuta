//! A handful of allocations per line, not one per letter or per word looked up. (Alone in its
//! test binary: the counter is shared by every thread.)

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lyrics_text::{Line, Word, romanize};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

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
fn a_few_allocations_per_line() {
    for line in LINES {
        let words = timed(line);
        romanize(&Line::new(line).words(&words));
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let r = romanize(&Line::new(line).words(&words)).expect("romanized");
        let used = ALLOCATIONS.load(Ordering::Relaxed) - before;
        // The text, its spans, the language tag, the word ranges, the words and one string per
        // word; a vector may grow once or twice.
        let budget = 8 + 2 * r.words.len();
        assert!(
            used <= budget,
            "{line}: {used} allocations, budget {budget}"
        );
    }
}
