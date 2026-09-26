//! 32-bit tag bitmask engine modeled after river-classic.

pub type TagMask = u32;

pub const TAG_NONE: TagMask = 0;
pub const TAG_ALL: TagMask = u32::MAX;
pub const TAG_1: TagMask = 1 << 0;
pub const TAG_SCRATCHPAD: TagMask = 1 << 31; // Tag 32

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagState {
    pub focused: TagMask,
    pub occupied: TagMask,
    pub urgent: TagMask,
}

impl Default for TagState {
    fn default() -> Self {
        Self::new()
    }
}

impl TagState {
    pub fn new() -> Self {
        Self {
            focused: TAG_1,
            occupied: TAG_NONE,
            urgent: TAG_NONE,
        }
    }

    #[inline]
    pub fn tag_index_to_mask(index: u8) -> TagMask {
        if index == 0 || index > 32 {
            TAG_NONE
        } else {
            1 << (index - 1)
        }
    }

    #[inline]
    pub fn is_view_visible(&self, view_tags: TagMask) -> bool {
        (view_tags & self.focused) != TAG_NONE
    }

    pub fn set_focused_tags(&mut self, mask: TagMask) {
        if mask != TAG_NONE {
            self.focused = mask;
        }
    }

    pub fn toggle_focused_tags(&mut self, mask: TagMask) {
        let new_focused = self.focused ^ mask;
        if new_focused != TAG_NONE {
            self.focused = new_focused;
        }
    }

    pub fn update_occupied_tags(&mut self, all_view_tags: &[TagMask]) {
        let mut mask = TAG_NONE;
        for &t in all_view_tags {
            mask |= t;
        }
        self.occupied = mask;
    }

    pub fn focused_tag_indices(&self) -> Vec<u8> {
        (1..=32)
            .filter(|&i| (self.focused & Self::tag_index_to_mask(i)) != TAG_NONE)
            .collect()
    }

    pub fn occupied_tag_indices(&self) -> Vec<u8> {
        (1..=32)
            .filter(|&i| (self.occupied & Self::tag_index_to_mask(i)) != TAG_NONE)
            .collect()
    }
}

#[cfg(test)]
mod tests;
