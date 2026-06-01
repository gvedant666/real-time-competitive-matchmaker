use super::primitives::Player;

pub const ARENA_CAPACITY: usize = 100_000;

// fixed size arena
// free list and LIFO for O(1) insertion
// maximizes cache locality for active players by reusing recently freed slots first
#[derive(Debug)]
pub struct Arena {
    slots: Vec<Option<Player>>,
    free_indices: Vec<usize>,
}

impl Arena {
    // Initializes the arena with all slots set to none
    // and the free list pre-populated with all indices in reverse order
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

    /// returns a shared reference to the player at the given index.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequential_insert() {
        let mut arena = Arena::new();
        
        let idx1 = arena.insert(Player::new(1000)).expect("Arena should not be full");
        let idx2 = arena.insert(Player::new(1001)).expect("Arena should not be full");
        let idx3 = arena.insert(Player::new(1002)).expect("Arena should not be full");
        
        // Verify that indices are given out sequentially starting from 0
        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 2);
        
        assert_eq!(arena.get(1).unwrap().mmr, 1001);
    }

    #[test]
    fn test_lifo_cache_locality() {
        let mut arena = Arena::new();
        
        // Allocate slot 0
        let idx1 = arena.insert(Player::new(1500)).unwrap();
        assert_eq!(idx1, 0);
        
        // Free slot 0, pushing it back onto the top of the LIFO stack
        arena.free(idx1);
        
        // The very next insert must immediately reuse slot 0
        let idx2 = arena.insert(Player::new(1600)).unwrap();
        assert_eq!(idx2, 0);
        assert_eq!(arena.get(0).unwrap().mmr, 1600);
    }

    #[test]
    fn test_arena_full_condition() {
        let mut arena = Arena::new();
        
        // Manually drain the free list to simulate a full Arena without 
        // spending time/memory allocating 100,000 actual test players.
        arena.free_indices.clear();
        
        let result = arena.insert(Player::new(2000));
        assert!(result.is_none(), "Arena should return None when the free list is empty");
    }
}