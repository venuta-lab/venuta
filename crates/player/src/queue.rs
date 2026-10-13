//! What plays next: the queue, its order, shuffle and repeat. Pure logic, no audio.

use rand::seq::SliceRandom;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Off,
    /// Start again from the top after the last song.
    All,
    /// The same song again and again.
    One,
}

/// The songs to play, in play order, and which one is current.
#[derive(Debug, Clone)]
pub struct Queue<T> {
    items: Vec<T>,
    /// The order before shuffling, to go back to it (positions into `items` at shuffle time
    /// are not kept: `unshuffled` holds the items themselves, by identity index).
    original: Option<Vec<usize>>,
    /// Identity of each item (stable across reorders), for unshuffling.
    ids: Vec<usize>,
    next_id: usize,
    pub index: Option<usize>,
    pub repeat: Repeat,
}

impl<T: Clone> Default for Queue<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            original: None,
            ids: Vec::new(),
            next_id: 0,
            index: None,
            repeat: Repeat::Off,
        }
    }
}

impl<T: Clone> Queue<T> {
    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn current(&self) -> Option<&T> {
        self.index.and_then(|i| self.items.get(i))
    }

    pub fn shuffled(&self) -> bool {
        self.original.is_some()
    }

    fn ids_for(&mut self, n: usize) -> Vec<usize> {
        let ids = (self.next_id..self.next_id + n).collect();
        self.next_id += n;
        ids
    }

    /// Replace everything; `index` is where to start (clamped).
    pub fn set(&mut self, items: Vec<T>, index: usize) {
        self.index = if items.is_empty() {
            None
        } else {
            Some(index.min(items.len() - 1))
        };
        self.ids = self.ids_for(items.len());
        self.items = items;
        self.original = None;
    }

    /// The song after the current one, following repeat (`One` repeats the current song).
    pub fn peek_next(&self) -> Option<usize> {
        let i = self.index?;
        match self.repeat {
            Repeat::One => Some(i),
            _ if i + 1 < self.items.len() => Some(i + 1),
            Repeat::All if !self.items.is_empty() => Some(0),
            _ => None,
        }
    }

    /// Move on after a song ends naturally.
    pub fn advance(&mut self) -> Option<usize> {
        let n = self.peek_next();
        self.index = n.or(self.index);
        n
    }

    /// Skip forward by hand: repeat-one doesn't hold you on the same song.
    pub fn skip_next(&mut self) -> Option<usize> {
        let i = self.index?;
        let n = if i + 1 < self.items.len() {
            Some(i + 1)
        } else if self.repeat == Repeat::All && !self.items.is_empty() {
            Some(0)
        } else {
            None
        };
        if n.is_some() {
            self.index = n;
        }
        n
    }

    pub fn skip_previous(&mut self) -> Option<usize> {
        let i = self.index?;
        let p = if i > 0 {
            Some(i - 1)
        } else if self.repeat == Repeat::All && !self.items.is_empty() {
            Some(self.items.len() - 1)
        } else {
            None
        };
        if p.is_some() {
            self.index = p;
        }
        p
    }

    pub fn jump(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.index = Some(index);
            true
        } else {
            false
        }
    }

    /// Shuffle what comes after the current song (the current one stays and plays on).
    /// Unshuffling puts the original order back, still on the same song.
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffled() {
            return;
        }
        if on {
            self.original = Some(self.ids.clone());
            // Put the current song first, then everything else in random order.
            let mut rest: Vec<usize> = (0..self.items.len())
                .filter(|&k| Some(k) != self.index)
                .collect();
            rest.shuffle(&mut rand::rng());
            let mut order: Vec<usize> = self.index.into_iter().collect();
            order.extend(rest);
            self.reorder(&order);
            self.index = self.index.map(|_| 0);
        } else if let Some(orig) = self.original.take() {
            let cur = self.index.map(|i| self.ids[i]);
            // Items added while shuffled keep their place at the end.
            let mut order: Vec<usize> = orig
                .iter()
                .filter_map(|id| self.ids.iter().position(|x| x == id))
                .collect();
            for k in 0..self.items.len() {
                if !order.contains(&k) {
                    order.push(k);
                }
            }
            self.reorder(&order);
            self.index = cur.and_then(|id| self.ids.iter().position(|x| *x == id));
        }
    }

    fn reorder(&mut self, order: &[usize]) {
        self.items = order.iter().map(|&k| self.items[k].clone()).collect();
        self.ids = order.iter().map(|&k| self.ids[k]).collect();
    }

    /// Add songs right after the current one (`next`) or at the end.
    pub fn insert(&mut self, items: Vec<T>, next: bool) {
        let at = if next {
            self.index.map(|i| i + 1).unwrap_or(0)
        } else {
            self.items.len()
        };
        let ids = self.ids_for(items.len());
        let n = items.len();
        for (k, (it, id)) in items.into_iter().zip(ids).enumerate() {
            self.items.insert(at + k, it);
            self.ids.insert(at + k, id);
        }
        if self.index.is_none() && n > 0 {
            self.index = Some(0);
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.items.len() {
            return None;
        }
        let it = self.items.remove(index);
        self.ids.remove(index);
        self.index = match self.index {
            _ if self.items.is_empty() => None,
            Some(i) if index < i => Some(i - 1),
            Some(i) if index == i => Some(i.min(self.items.len() - 1)),
            other => other,
        };
        Some(it)
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let it = self.items.remove(from);
        let id = self.ids.remove(from);
        self.items.insert(to, it);
        self.ids.insert(to, id);
        if let Some(i) = self.index {
            self.index = Some(if i == from {
                to
            } else if from < i && to >= i {
                i - 1
            } else if from > i && to <= i {
                i + 1
            } else {
                i
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: usize, at: usize) -> Queue<usize> {
        let mut q = Queue::default();
        q.set((0..n).collect(), at);
        q
    }

    #[test]
    fn plays_through_and_stops_or_repeats() {
        let mut a = q(3, 1);
        assert_eq!(a.advance(), Some(2));
        assert_eq!(a.advance(), None, "end of the queue");
        assert_eq!(a.index, Some(2));
        a.repeat = Repeat::All;
        assert_eq!(a.advance(), Some(0));
        a.repeat = Repeat::One;
        assert_eq!(a.advance(), Some(0), "the same song again");
        assert_eq!(
            a.skip_next(),
            Some(1),
            "skipping by hand moves on even on repeat one"
        );
        assert_eq!(a.skip_previous(), Some(0));
    }

    #[test]
    fn shuffle_keeps_the_current_song_and_unshuffle_restores_the_order() {
        let mut a = q(20, 7);
        a.set_shuffle(true);
        assert_eq!(a.current(), Some(&7));
        assert_eq!(a.index, Some(0));
        let mut seen = a.items().to_vec();
        seen.sort();
        assert_eq!(
            seen,
            (0..20).collect::<Vec<_>>(),
            "every song still there once"
        );
        a.skip_next();
        let now = *a.current().unwrap();
        a.set_shuffle(false);
        assert_eq!(a.items(), (0..20).collect::<Vec<_>>().as_slice());
        assert_eq!(a.current(), Some(&now), "still on the same song");
    }

    #[test]
    fn insert_remove_and_move_keep_the_current_song() {
        let mut a = q(4, 1); // [0 1 2 3], on 1
        a.insert(vec![9], true); // [0 1 9 2 3]
        assert_eq!(a.items(), &[0, 1, 9, 2, 3]);
        assert_eq!(a.current(), Some(&1));
        a.insert(vec![8], false);
        assert_eq!(a.items().last(), Some(&8));
        a.remove(0); // [1 9 2 3 8]
        assert_eq!(a.current(), Some(&1));
        a.move_item(0, 3); // [9 2 3 1 8]
        assert_eq!(a.current(), Some(&1));
        assert_eq!(a.index, Some(3));
        a.move_item(4, 0); // [8 9 2 3 1]
        assert_eq!(a.index, Some(4));
        assert_eq!(a.remove(4), Some(1));
        assert_eq!(
            a.current(),
            Some(&3),
            "removing the current song moves to the one now in its place"
        );
    }
}
