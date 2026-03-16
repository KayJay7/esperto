use crate::combo::types::{Combo, Group, Key, Range};
use crate::config::Config;
use crate::types::Keycode;
use crate::types::{Event, Kind};
use frozen_collections::FzScalarMap;
use std::collections::{HashMap, HashSet, VecDeque};
use tinyset::SetUsize;

pub use types::Queue;

mod types;

const EVENT_BUFFER_WARMUP: usize = 16;

/// This provides the main functionalities of the library.
/// It is generic in the input and output keycode types, but it requires
/// that they implement the [`Keycode`] trait, which includes the [`Copy`] trait.
///
/// If your events are need to be heap allocated types (that are not [`Copy`]),
/// consider storing them on an indexable collection, and use the indices as keycodes.
/// Consider using the methods [`Config::map_input`], [`Config::map_output`],
/// and [`Config::iter_actions`] to help with the conversion.
pub struct ComboHandler<A: Keycode, Z: Keycode, Q: Queue<Event<Z>>> {
   // precomputed
   domain: FzScalarMap<A, usize>,  // keycode to key index
   keys: Box<[Key<Z>]>,            // keys
   keys_combos: Box<[Combo<Z>]>,   // optimization: packed key combos
   keys_groups: Box<[usize]>,      // optimization: packed key groups
   groups: Box<[Group]>,           // modifier groups
   groups_keys: Box<[usize]>,      // optimization: packed group keys
   groups_pred: Box<[usize]>,      // optimization: packed group pred
   groups_intersect: Box<[usize]>, // optimization: packed group intersect
   // dynamic
   masks: i32,         // #active masks
   cache_counter: i32, // current cache key
   /// Output event queue. This is filled when calling the [`ComboHandler::handle`] method.
   /// The queue is populated using the [`Queue::push`] method. When created using [`ComboHandler::new`], the queue
   /// is of type [`VecDeque`], use the method [`VecDeque::pop_front`] to extract the output events.
   pub events: Q, // output event queue
}

impl<A: Keycode, Z: Keycode> ComboHandler<A, Z, VecDeque<Event<Z>>> {
   /// Creates the handler object from a configuration object, using a [`VecDeque`]
   /// as event queue. The queue pre-allocates some capacity, to possibly avoid
   /// allocations during event handling.
   ///
   /// This method does a lot precomputation in order to speed up subsequent calls to
   /// the [`ComboHandler::handle`] method. It will be slow on complex configurations.
   pub fn new(config: &Config<A, Z>) -> ComboHandler<A, Z, VecDeque<Event<Z>>> {
      ComboHandler::with(config, VecDeque::with_capacity(EVENT_BUFFER_WARMUP))
   }
}

impl<A: Keycode, Z: Keycode, Q: Queue<Event<Z>>> ComboHandler<A, Z, Q> {
   fn is_masking(&self) -> bool {
      self.masks > 0
   }

   /// Creates the handler object from a configuration object, using the provided queue.
   ///
   /// This method does a lot precomputation in order to speed up subsequent calls to
   /// the [`ComboHandler::handle`] method. It will be slow on complex configurations.
   pub fn with(config: &Config<A, Z>, queue: Q) -> ComboHandler<A, Z, Q> {
      struct MutKey<B: Keycode> {
         action: Option<B>,
         latching: bool,
         immediate: bool,
         combos: Vec<Combo<B>>,
         groups: Vec<usize>,
      }

      impl<B: Keycode> Default for MutKey<B> {
         fn default() -> Self {
            Self {
               action: None,
               latching: false,
               immediate: false,
               combos: vec![],
               groups: vec![],
            }
         }
      }

      impl<B: Keycode> MutKey<B> {
         fn freeze(
            mut self,
            groups: &[Group],
            keys_combos: &mut Vec<Combo<B>>,
            keys_groups: &mut Vec<usize>,
         ) -> Key<B> {
            self
               .combos
               .sort_unstable_by(|x, y| groups[y.group].size.cmp(&groups[x.group].size));
            let combos_start = keys_combos.len();
            keys_combos.extend(self.combos);
            let combos_end = keys_combos.len();

            let groups_start = keys_groups.len();
            keys_groups.extend(self.groups);
            let groups_end = keys_groups.len();

            Key {
               action: self.action,
               latching: self.latching,
               immediate: self.immediate,
               combos: Range::new(combos_start, combos_end),
               groups: Range::new(groups_start, groups_end),
               cache_counter: 0,
               open: false,
               active_combo: None,
            }
         }
      }

      struct MutGroup {
         index: usize,
         mask: bool,
         greater: Vec<usize>,
         pred: Vec<usize>,
         intersect: Vec<usize>,
         keys: Vec<usize>,
      }

      impl MutGroup {
         fn freeze(
            self,
            groups_pred: &mut Vec<usize>,
            groups_intersect: &mut Vec<usize>,
            groups_keys: &mut Vec<usize>,
         ) -> Group {
            let pred_start = groups_pred.len();
            groups_pred.extend(self.pred);
            let pred_end = groups_pred.len();

            let intersect_start = groups_intersect.len();
            groups_intersect.extend(self.intersect);
            let intersect_end = groups_intersect.len();

            let keys_start = groups_keys.len();
            groups_keys.extend(self.keys);
            let keys_end = groups_keys.len();
            let keys = Range::new(keys_start, keys_end);

            Group {
               index: self.index,
               mask: self.mask,
               greater: self.greater.into_iter().collect(),
               pred: Range::new(pred_start, pred_end),
               intersect: Range::new(intersect_start, intersect_end),
               keys,
               size: keys.len(),
               active_combos: SetUsize::new(),
               counter: 0,
               active_greater: 0,
               mask_weight: 0,
            }
         }
      }

      // graph build
      let (named_groups, groups): (HashMap<String, usize>, Vec<HashSet<A>>) = config
         .modifiers
         .iter()
         .enumerate()
         .map(|(i, modifier_decl)| ((modifier_decl.id.clone(), i), modifier_decl.keys.clone()))
         .unzip();
      let mut edges = vec![(vec![], vec![], vec![]); groups.len()];
      for (a_index, a) in groups.iter().enumerate() {
         for (b_index, b) in groups.iter().enumerate() {
            if a_index == b_index || a.is_disjoint(b) || a.is_superset(b) {
               // ignore self loops and symmetry
               continue;
            }
            if a.is_subset(b) {
               // a ⊆ b
               edges[a_index].0.push(b_index);

               if !edges[b_index]
                  .1
                  .iter()
                  .any(|below: &usize| groups[*below].is_superset(a))
               {
                  // b ∈ succ(a)
                  edges[b_index]
                     .1
                     // drop all belows ⊆ a
                     .retain(|below| !groups[*below].is_subset(a));
                  edges[b_index].1.push(a_index);
               }
               continue;
            }
            // unordered intersection
            edges[a_index].2.push(b_index);
         }
      }

      let mut domain: HashMap<A, usize> = HashMap::new();
      let mut temp_keys: Vec<MutKey<Z>> = vec![];
      // domain: populate modifiers
      for (i, group) in groups.into_iter().enumerate() {
         for keycode in group {
            if let Some(key) = domain.get(&keycode) {
               temp_keys[*key].groups.push(i);
            } else {
               domain.insert(keycode, temp_keys.len());
               let mut temp_key = MutKey::default();
               temp_key.groups.push(i);
               temp_keys.push(temp_key);
            }
         }
      }

      let mut groups_keys = vec![];
      let mut pred_adjacency = vec![];
      let mut intersect_adjacency = vec![];
      let mut groups: Box<[Group]> = edges
         .into_iter()
         .enumerate()
         .zip(config.modifiers.iter())
         .map(|((index, (above, below, intersect)), modifier_decl)| {
            // collect modifier keys
            let mut keys = Vec::new();
            for key in &modifier_decl.keys {
               keys.push(domain[&key]);
            }
            MutGroup {
               index,
               mask: modifier_decl.masking,
               greater: above,
               pred: below,
               intersect,
               keys,
            }
         })
         .map(|group| group.freeze(&mut pred_adjacency, &mut intersect_adjacency, &mut groups_keys))
         .collect();

      for group in 0..groups.len() {
         groups[group].mask_weight = groups[group].mask as i32
            - groups[group]
               .iter_pred(&pred_adjacency)
               .map(|group| groups[*group].mask as i32)
               .sum::<i32>();
      }

      // domain: populate action keys
      for action in config.actions.iter() {
         let temp_key: &mut MutKey<Z>;
         if let Some(i) = domain.get(&action.key) {
            temp_key = &mut temp_keys[*i];
         } else {
            let i = temp_keys.len();
            domain.insert(action.key, i);
            temp_keys.push(MutKey::default());
            temp_key = &mut temp_keys[i];
         }

         temp_key.immediate = action.immediate;
         temp_key.latching = action.latching;
         temp_key.action = action.action;
         for combo in &action.modified {
            temp_key.combos.insert(
               temp_key
                  .combos
                  .partition_point(|x| groups[x.group] <= groups[named_groups[&combo.modifier]]),
               Combo {
                  action: combo.action,
                  group: named_groups[&combo.modifier],
               },
            )
         }
      }
      let mut keys_combos = vec![];
      let mut keys_groups = vec![];

      ComboHandler {
         domain: FzScalarMap::new(domain.into_iter().collect()),
         keys: temp_keys
            .into_iter()
            .map(|key| key.freeze(&groups, &mut keys_combos, &mut keys_groups))
            .collect(),
         keys_combos: keys_combos.into_boxed_slice(),
         keys_groups: keys_groups.into_boxed_slice(),
         groups,
         groups_keys: groups_keys.into_boxed_slice(),
         groups_pred: pred_adjacency.into_boxed_slice(),
         groups_intersect: intersect_adjacency.into_boxed_slice(),
         masks: 0,
         cache_counter: 1,
         events: queue,
      }
   }

   /// Handles an input event. The method returns:
   ///
   /// * `true` if the input event was handled (the keycode was mentioned in the configuration)
   /// * `false` it the input event was not handled (the keycode wasn't in the configuration)
   ///
   /// Events that are not handled do not produce any output events.
   ///
   /// The method expects a "sane" event sequence (i.e. no double-keydown or double-keyup).
   /// The behaviour for non-sane sequences is undefined.
   ///
   /// Output events are not returned, but pushed *in order* on the `events` field.
   /// If the event queue is not empty when calling this method, it is **not** cleared
   /// and new events are added to the queue. To avoid (possibly costly) memory allocations
   /// it is advised that you handle all output events before calling this method, so the queue
   /// doesn't need to grow to accommodate for the new events.
   pub fn handle(&mut self, event: Event<A>) -> bool {
      let key = *if let Some(key) = self.domain.get(&event.keycode) {
         key
      } else {
         return false;
      };
      match event.kind {
         Kind::Down | Kind::Axis => {
            let mut invalidate_cache = false;
            self.keys[key].open();

            if event.kind == Kind::Down {
               // modifier key
               for group in self.keys[key].iter_groups(&self.keys_groups) {
                  // increase group counter
                  self.groups[*group].counter += 1;
                  if self.groups[*group].is_active() {
                     // for every just activated group
                     self.masks += self.groups[*group].mask_weight;
                     invalidate_cache = true;
                     if self.groups[*group].keys.len() > 1 {
                        // singletons do not close themselves
                        for key in self.groups[*group].iter_keys(&self.groups_keys) {
                           // close all delayed modifier keys
                           self.keys[*key].open &= self.keys[*key].is_immediate();
                        }
                     }
                     for group in self.groups[*group].iter_pred(&self.groups_pred) {
                        self.groups[*group].active_greater += 1;
                        close_active_combos(
                           &mut self.groups[*group],
                           &mut self.keys,
                           &self.keys_combos,
                           &mut self.events,
                        );
                     }
                  }
               }
            } else {
               self.keys[key].immediate = true;
               self.keys[key].latching = true;
            }

            self.invalidate_cache(invalidate_cache);

            // optimization: skip conflict resolution on closed keyup modifier keys
            if !self.keys[key].is_immediate() && !self.keys[key].open {
               return true;
            }

            self.keys[key].open &= !self.is_masking();

            if self.keys[key].cache_counter == self.cache_counter {
               if self.keys[key].is_immediate() {
                  self.keys[key].open();
                  self.keys[key]
                     .active_combo
                     .and_then(|i| {
                        let combo = self.keys[key].get_combo(i, &self.keys_combos);
                        if !self.keys[key].latching {
                           self.groups[combo.group].active_combos.insert(key);
                        }
                        combo.action
                     })
                     .or(self.keys[key].action.filter(|_| !self.is_masking()))
                     .map(|action| {
                        self.events.push(Event {
                           keycode: action,
                           kind: event.kind,
                           value: event.value,
                        })
                     });
               }
               return true;
            }
            self.keys[key].cache_counter = self.cache_counter;

            // action key
            let combos = self.keys[key].combos.len();
            let mut i = self.keys[key]
               .iter_combos(&self.keys_combos)
               .position(|combo| self.groups[combo.group].is_active())
               .unwrap_or(combos);
            if i == combos {
               // not modified
               self.maybe_action(key, event.kind, event.value);
               return true;
            }

            let candidate_combo = i;
            let candidate_group = self.keys[key].get_combo(candidate_combo, &self.keys_combos).group;

            // search action key conflicts
            while i < combos {
               let i_group = self.keys[key].get_combo(i, &self.keys_combos).group;
               if self.groups[i_group].is_active() && !(self.groups[i_group] <= self.groups[candidate_group]) {
                  self.maybe_action(key, event.kind, event.value);
                  return true;
               }
               i += 1;
            }

            // search modifier key conflicts
            let conflict: bool = self.groups[candidate_group].is_shadowed() // no active supergroups
                    || self.groups[candidate_group]
                    .iter_intersect(&self.groups_intersect)
                    .any(|group| self.groups[*group].is_active()); // no active intersecting groups
            if conflict {
               self.maybe_action(key, event.kind, event.value);
               return true;
            }

            // singletons do not close themselves to allow delayed modifier keys
            if self.groups[candidate_group].size == 1 {
               for key in self.groups[candidate_group].iter_keys(&self.groups_keys) {
                  if !self.keys[*key].is_immediate() {
                     // immediate modifiers still got to send their keyup
                     self.keys[*key].close();
                  }
               }
            }

            // no conflicts activate combo
            if !self.keys[key].latching {
               self.groups[candidate_group].active_combos.insert(key);
            }
            if self.keys[key].is_immediate()
               && let Some(action) = self.keys[key].get_combo(candidate_combo, &self.keys_combos).action
            {
               self.events.push(Event {
                  keycode: action,
                  kind: event.kind,
                  value: event.value,
               });
               self.keys[key].open();
            }
            self.keys[key].active_combo = Some(candidate_combo);
         }
         Kind::Up => {
            let mut invalidate_cache = false;
            for group in self.keys[key].iter_groups(&self.keys_groups) {
               if self.groups[*group].is_active() {
                  for group in self.groups[*group].iter_pred(&self.groups_pred) {
                     self.groups[*group].active_greater -= 1;
                  }
                  invalidate_cache = true;
                  self.masks -= self.groups[*group].mask_weight;
                  close_active_combos(
                     &mut self.groups[*group],
                     &mut self.keys,
                     &self.keys_combos,
                     &mut self.events,
                  );
               }
               self.groups[*group].counter -= 1;
            }

            self.invalidate_cache(invalidate_cache);

            if self.keys[key].open {
               self.keys[key]
                  .active_combo
                  .and_then(|i| {
                     let combo = self.keys[key].get_combo(i, &self.keys_combos);
                     self.groups[combo.group].active_combos.remove(key);
                     combo.action
                  })
                  .or(self.keys[key].action)
                  .map(|action| {
                     if !self.keys[key].is_immediate() {
                        self.events.push(Event {
                           keycode: action,
                           kind: Kind::Down,
                           value: 0,
                        });
                     }
                     self.events.push(Event {
                        keycode: action,
                        kind: Kind::Up,
                        value: 0,
                     })
                  });
            }
         }
      }
      true
   }

   fn invalidate_cache(&mut self, invalidate_cache: bool) {
      self.cache_counter = self.cache_counter.wrapping_add(invalidate_cache as i32);
   }

   fn maybe_action(&mut self, key: usize, kind: Kind, value: i16) {
      if !self.is_masking()
         && self.keys[key].is_immediate()
         && let Some(action) = self.keys[key].action
      {
         self.events.push(Event {
            keycode: action,
            kind,
            value,
         });
         self.keys[key].open();
      }
      self.keys[key].active_combo = None;
   }
}

impl<A: Keycode, Q: Queue<Event<A>>> ComboHandler<A, A, Q> {
   /// Like [`ComboHandler::handle`], but unhandled events are pushed directly
   /// to the output events queue. The method returns:
   ///
   /// * `true` if the event was handled
   /// * `false` if the event was not handled
   ///
   /// This method is only available when input and output keycode types are the same.
   pub fn handle_passthrough(&mut self, event: Event<A>) -> bool {
      if !self.handle(event) {
         self.events.push(event);
         return false;
      }
      true
   }
}

fn close_active_combos<Z: Keycode>(
   group: &mut Group,
   keys: &mut [Key<Z>],
   keys_combos: &[Combo<Z>],
   events: &mut impl Queue<Event<Z>>,
) {
   for key in group.active_combos.drain() {
      // terminate the actions it modified
      keys[key].close();
      if keys[key].is_immediate()
         && let Some(action) = keys[key]
            .active_combo
            .and_then(|combo| keys[key].get_combo(combo, keys_combos).action)
      {
         // keyup modifiers did not produce a keydown
         events.push(Event {
            keycode: action,
            kind: Kind::Up,
            value: 0,
         });
      }
   }
}
