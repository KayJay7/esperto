use crate::types::Keycode;
use frozen_collections::FzScalarSet;
use std::cmp::{max, Ordering};
use std::collections::VecDeque;
use tinyset::SetUsize;

#[derive(Debug, Clone)]
pub struct Group {
   // precomputed
   pub index: usize,                // index of self (for partial ordering)
   pub mask: bool,                  // masking flag
   pub greater: FzScalarSet<usize>, // supergroups
   pub pred: Range,                 // neighbouring subgroups
   pub intersect: Range,            // partial intersectors
   pub keys: Range,                 // modifier keys
   pub size: usize,                 // #modifier keys
   pub active_combos: SetUsize,     // currently down combos
   // dynamic
   pub counter: usize,      // #currently down modifier keys
   pub active_greater: i32, // #currently active supergroups
   pub mask_weight: i32,    // (1?)-#masking subgroups
}

impl Group {
   pub fn is_active(&self) -> bool {
      self.counter == self.size
   }

   pub fn is_shadowed(&self) -> bool {
      self.active_greater > 0
   }

   pub fn iter_intersect<'a>(&self, groups_intersect: &'a [usize]) -> impl Iterator<Item = &'a usize> + use<'a> {
      self.intersect.into_iter().map(|i| &groups_intersect[i])
   }

   pub fn iter_pred<'a>(&self, groups_pred: &'a [usize]) -> impl Iterator<Item = &'a usize> + use<'a> {
      self.pred.into_iter().map(|i| &groups_pred[i])
   }

   pub fn iter_keys<'a>(&self, groups_keys: &'a [usize]) -> impl Iterator<Item = &'a usize> + use<'a> {
      self.keys.into_iter().map(|i| &groups_keys[i])
   }
}
impl PartialEq for Group {
   fn eq(&self, other: &Self) -> bool {
      self.index == other.index
   }
}
impl PartialOrd for Group {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      if self == other {
         return Some(Ordering::Equal);
      }
      if self.greater.contains(&other.index) {
         return Some(Ordering::Less);
      }
      if other.greater.contains(&self.index) {
         return Some(Ordering::Greater);
      }
      None
   }
}

#[derive(Debug, Clone)]
pub struct Key<Z: Keycode> {
   // precomputed
   // key: Keycode,              // validate mphf
   pub action: Option<Z>,  // action key: unmodified action
   pub latching: bool,     // action key: after modifier deactivation
   pub immediate: bool,    // modifier key: keydown immediately
   pub combos: Range,      // action key: modified mappings
   pub groups: Range,      // modifier key: superset modifier groups
   pub cache_counter: i32, // action key: cache key
   // dynamic
   pub open: bool,                  // requires keyup handling
   pub active_combo: Option<usize>, // action key: active action
}
impl<Z: Keycode> Key<Z> {
   pub fn is_modifier(&self) -> bool {
      !self.groups.is_empty()
   }

   pub fn is_immediate(&self) -> bool {
      !self.is_modifier() || self.immediate
   }

   pub fn iter_combos<'a>(&self, keys_combos: &'a [Combo<Z>]) -> impl Iterator<Item = Combo<Z>> + use<'a, Z> {
      self.combos.into_iter().map(|i| keys_combos[i])
   }

   pub fn iter_groups<'a>(&self, keys_groups: &'a [usize]) -> impl Iterator<Item = &'a usize> + use<'a, Z> {
      self.groups.into_iter().map(|i| &keys_groups[i])
   }

   pub fn get_combo(&self, index: usize, keys_combos: &[Combo<Z>]) -> Combo<Z> {
      keys_combos[self.combos.ind(index)]
   }

   pub fn close(&mut self) {
      self.open = false
   }

   pub fn open(&mut self) {
      self.open = true;
   }
}

#[derive(Debug, Clone, Copy)]
pub struct Combo<Z: Keycode> {
   pub action: Option<Z>, // target action
   pub group: usize,      // modifier group index
}

#[derive(Debug, Clone, Copy)]
pub struct Range {
   start: usize,
   end: usize,
}

impl Range {
   pub fn new(start: usize, end: usize) -> Range {
      Range { start, end }
   }

   pub fn is_empty(&self) -> bool {
      self.end <= self.start
   }

   pub fn len(&self) -> usize {
      max(0, self.end - self.start)
   }

   pub fn ind(&self, index: usize) -> usize {
      assert!(index < self.len());
      self.start + index
   }
}

impl IntoIterator for Range {
   type Item = usize;
   type IntoIter = std::ops::Range<usize>;

   fn into_iter(self) -> Self::IntoIter {
      self.start..self.end
   }
}

/// Trait for the output event queue.
pub trait Queue<T> {
   fn push(&mut self, value: T);
}

impl<T> Queue<T> for VecDeque<T> {
   fn push(&mut self, value: T) {
      self.push_back(value)
   }
}

impl<T> Queue<T> for Vec<T> {
   fn push(&mut self, value: T) {
      Vec::push(self, value)
   }
}
