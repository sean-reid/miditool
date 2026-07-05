//! Per-note state grids, the building block for effects that must remember
//! what they did to a note-on so the matching note-off lands correctly.

/// A value per (channel, key). 16 x 128 slots, fixed size, no heap.
#[derive(Debug, Clone)]
pub struct PerNote<T>([[T; 128]; 16]);

impl<T: Copy + Default> Default for PerNote<T> {
    fn default() -> Self {
        Self([[T::default(); 128]; 16])
    }
}

impl<T: Copy + Default> PerNote<T> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn get(&self, ch: u8, key: u8) -> T {
        self.0[ch as usize & 15][key as usize & 127]
    }

    #[inline]
    pub fn set(&mut self, ch: u8, key: u8, value: T) {
        self.0[ch as usize & 15][key as usize & 127] = value;
    }

    /// Replace with the default value, returning the previous one.
    #[inline]
    pub fn take(&mut self, ch: u8, key: u8) -> T {
        std::mem::take(&mut self.0[ch as usize & 15][key as usize & 127])
    }

    /// Visit every non-default slot. `T: PartialEq` bound kept implicit by
    /// requiring callers to filter; iterates all slots.
    pub fn for_each(&self, mut f: impl FnMut(u8, u8, T)) {
        for ch in 0..16u8 {
            for key in 0..128u8 {
                f(ch, key, self.get(ch, key));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_take() {
        let mut m: PerNote<Option<u8>> = PerNote::new();
        assert_eq!(m.get(3, 60), None);
        m.set(3, 60, Some(72));
        assert_eq!(m.get(3, 60), Some(72));
        assert_eq!(m.take(3, 60), Some(72));
        assert_eq!(m.get(3, 60), None);
    }
}
