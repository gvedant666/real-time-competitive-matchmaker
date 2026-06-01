use super::primitives::Player;

pub const ARENA_CAPACITY: usize = 100_000;

#[derive(Debug)]
pub struct Arena {
    slots: Vec<Option<Player>>,
    free_indices: Vec<usize>,
}

impl Arena {
    pub fn new() -> Self {
        
        let slots = std::iter::repeat_with(|| None)
            .take(ARENA_CAPACITY)
            .collect();

        // pre-allocate the free list to avoid reallocations
        let mut free_indices = Vec::with_capacity(ARENA_CAPACITY);
        
        // push in reverse order
        for i in (0..ARENA_CAPACITY).rev() {
            free_indices.push(i);
        }

        Self {
            slots,
            free_indices,
        }
    }

    // pop and push indexes
    #[inline(always)]
    pub fn insert(&mut self, player: Player) -> Option<usize> {
        let index = self.free_indices.pop()?;
        
        self.slots[index] = Some(player);
        Some(index)
    }

    // returns a shared reference to the player at the given index.
    #[inline(always)]
    pub fn get(&self, index: usize) -> Option<&Player> {
        self.slots.get(index)?.as_ref()
    }

    // free a slot and push the index back
    #[inline(always)]
    pub fn free(&mut self, index: usize) {
        if let Some(slot) = self.slots.get_mut(index) {
            if slot.is_some() {
                *slot = None;
                self.free_indices.push(index);
            }
        }
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}