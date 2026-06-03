use super::primitives::Player;

pub const ARENA_CAPACITY: usize = 100_000;

#[derive(Debug)]
pub struct Arena {
    slots: Vec<Option<Player>>,
    free_indices: Vec<usize>,
}

impl Arena {
    pub fn new() -> Self {
        let mut slots = Vec::with_capacity(ARENA_CAPACITY);
        for _ in 0..ARENA_CAPACITY {
            slots.push(None);
        }

        let mut free_indices = Vec::with_capacity(ARENA_CAPACITY);
        for i in (0..ARENA_CAPACITY).rev() {
            free_indices.push(i);
        }

        Self {
            slots,
            free_indices,
        }
    }

    #[inline(always)]
    pub fn insert(&mut self, player: Player) -> Option<usize> {
        let index = self.free_indices.pop()?;
        self.slots[index] = Some(player);
        Some(index)
    }

    #[inline(always)]
    pub fn get(&self, index: usize) -> Option<&Player> {
        self.slots.get(index)?.as_ref()
    }

    #[inline(always)]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Player> {
        self.slots.get_mut(index)?.as_mut()
    }

    // grab the player and recycle the slot instantly
    #[inline(always)]
    pub fn take(&mut self, index: usize) -> Option<Player> {
        if let Some(slot) = self.slots.get_mut(index) {
            if slot.is_some() {
                self.free_indices.push(index);
                return slot.take();
            }
        }
        None
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}