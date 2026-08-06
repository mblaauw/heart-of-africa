//! Tile level-of-detail selection and the tile cache (WS1 renderer split).
//!
//! WS1 draws the atlas via source rects, so per-frame tile textures are never
//! built; the LRU below exists for the WS2 LOD pyramid, which pages the map
//! from `hoatool gen-map` output. It is tested here and consumed once the
//! pyramid lands — do not delete it as dead code.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// LOD tiers. WS1 has two; the pyramid adds more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    /// The whole map as one scaled texture.
    Flat,
    /// Per-tile atlas draws.
    Atlas,
}

/// Pick the tile LOD for a zoom (tile pixel size). Below 20 px/tile the
/// atlas would need thousands of tiny draws — draw the whole map as one
/// scaled texture instead.
pub fn lod_for_tile_size(tile_size: f32) -> Lod {
    if tile_size < 20.0 {
        Lod::Flat
    } else {
        Lod::Atlas
    }
}

/// Fixed-capacity LRU keyed by packed tile coordinates. Evicts the least
/// recently used entry when full; `get` refreshes recency.
///
/// Not yet consumed by WS1 rendering (the atlas path builds no per-tile
/// textures); the WS2 pyramid pages the map and this cache keys them.
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct TileCache<K, V> {
    capacity: usize,
    order: VecDeque<K>,
    map: HashMap<K, V>,
}

impl<K: Eq + Hash + Clone, V> TileCache<K, V> {
    /// A cache holding at most `capacity` entries.
    pub fn new(capacity: usize) -> TileCache<K, V> {
        TileCache {
            capacity: capacity.max(1),
            order: VecDeque::with_capacity(capacity.max(1)),
            map: HashMap::with_capacity(capacity.max(1)),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Look up a key, refreshing its recency.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self.map.contains_key(key) {
            self.touch(key);
        }
        self.map.get(key)
    }

    /// Insert a value, evicting the least recently used entry if full.
    pub fn insert(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            self.touch(&key);
            return;
        }
        while self.map.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
    }

    /// Move `key` to the back of the recency queue.
    fn touch(&mut self, key: &K) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(pos).unwrap();
            self.order.push_back(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_threshold() {
        assert_eq!(lod_for_tile_size(19.99), Lod::Flat);
        assert_eq!(lod_for_tile_size(20.0), Lod::Atlas);
        assert_eq!(lod_for_tile_size(8.0), Lod::Flat);
        assert_eq!(lod_for_tile_size(64.0), Lod::Atlas);
    }

    #[test]
    fn lru_evicts_oldest_when_full() {
        let mut c = TileCache::new(2);
        c.insert((0, 0), "a");
        c.insert((1, 1), "b");
        c.insert((2, 2), "c");
        assert_eq!(c.len(), 2);
        assert!(c.get(&(0, 0)).is_none(), "oldest evicted");
        assert_eq!(c.get(&(1, 1)), Some(&"b"));
        assert_eq!(c.get(&(2, 2)), Some(&"c"));
    }

    #[test]
    fn get_refreshes_recency() {
        let mut c = TileCache::new(2);
        c.insert((0, 0), "a");
        c.insert((1, 1), "b");
        let _ = c.get(&(0, 0));
        c.insert((2, 2), "c");
        assert_eq!(c.get(&(0, 0)), Some(&"a"), "refreshed entry survives");
        assert!(c.get(&(1, 1)).is_none(), "stale entry evicted");
    }

    #[test]
    fn insert_overwrite_keeps_recency_position() {
        let mut c = TileCache::new(3);
        c.insert((0, 0), "a");
        c.insert((1, 1), "b");
        c.insert((0, 0), "a2");
        assert_eq!(c.get(&(0, 0)), Some(&"a2"));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn empty_and_capacity_one() {
        let mut c = TileCache::new(1);
        assert!(c.is_empty());
        c.insert((1, 1), "x");
        c.insert((2, 2), "y");
        assert_eq!(c.len(), 1);
        assert_eq!(c.get(&(2, 2)), Some(&"y"));
    }
}
