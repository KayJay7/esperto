use crate::types::ConfigValidationError::*;
use crate::types::ConfigValidationWarning::*;
use crate::types::{ConfigValidationError, ConfigValidationWarning};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;

/// Configuration object, can be serialized and deserialized if the feature `"serde"` is enabled.
/// If the generics implement [`crate::types::Keycode`], it can be used to create a [`crate::combo::ComboHandler`] object.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Config<A: Eq + Hash, Z: Eq + Hash> {
   /// List of modifier groups
   #[cfg_attr(feature = "serde", serde(default = "Vec::default"))]
   pub modifiers: Vec<ModifierDecl<A>>,
   /// List of actions
   #[cfg_attr(feature = "serde", serde(default = "Vec::default"))]
   pub actions: Vec<Action<A, Z>>,
}

impl<A: Eq + Hash, Z: Eq + Hash> Config<A, Z> {
   /// Remap the input keycodes, useful for converting types
   pub fn map_input<B: Eq + Hash>(self, mut f: impl FnMut(A) -> B) -> Config<B, Z> {
      Config {
         modifiers: self
            .modifiers
            .into_iter()
            .map(|group| ModifierDecl {
               id: group.id,
               keys: group.keys.into_iter().map(&mut f).collect(),
               masking: group.masking,
            })
            .collect(),
         actions: self
            .actions
            .into_iter()
            .map(|action| Action {
               key: f(action.key),
               action: action.action,
               immediate: action.immediate,
               modified: action.modified,
               latching: action.latching,
            })
            .collect(),
      }
   }

   /// Remap the output keycodes, useful for converting types
   pub fn map_output<Y: Eq + Hash>(self, mut f: impl FnMut(Z) -> Y) -> Config<A, Y> {
      Config {
         modifiers: self.modifiers,
         actions: self
            .actions
            .into_iter()
            .map(|action| Action {
               key: action.key,
               action: action.action.map(&mut f),
               immediate: action.immediate,
               modified: action
                  .modified
                  .into_iter()
                  .map(|combo| Combo {
                     modifier: combo.modifier,
                     action: combo.action.map(&mut f),
                  })
                  .collect(),
               latching: action.latching,
            })
            .collect(),
      }
   }

   /// Iterate over all actions and combos, yields a tuple of:
   ///
   /// * input keycode
   /// * optional modifier group id (`None` if not a combo)
   /// * (optional) output keycode
   pub fn iter_actions(&self) -> impl Iterator<Item = (&A, Option<&String>, &Option<Z>)> {
      self.actions.iter().flat_map(|action| {
         std::iter::once((&action.key, None, &action.action)).chain(
            action
               .modified
               .iter()
               .map(|combo| (&action.key, Some(&combo.modifier), &combo.action)),
         )
      })
   }
}

/// Action definition
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Action<A: Eq + Hash, Z: Eq + Hash> {
   /// Input action
   pub key: A,
   /// Optional output action
   #[cfg_attr(feature = "serde", serde(default = "Option::default"))]
   pub action: Option<Z>,
   /// `true` if the action is immediate
   #[cfg_attr(feature = "serde", serde(default = "bool::default"))]
   pub immediate: bool,
   /// List of combos for the input key
   #[cfg_attr(feature = "serde", serde(default = "Vec::default"))]
   pub modified: Vec<Combo<Z>>,
   /// `true` if the action is latching
   #[cfg_attr(feature = "serde", serde(default = "bool::default"))]
   pub latching: bool,
}

/// Combo declaration
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Combo<Z: Eq + Hash> {
   /// Modifier group id, as defined in [`ModifierDecl`]
   pub modifier: String,
   /// (Optional) output action
   pub action: Option<Z>,
}

/// Modifier group declaration
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ModifierDecl<A: Eq + Hash> {
   /// Id of the group
   #[cfg_attr(feature = "serde", serde(rename = "name"))]
   pub id: String,
   /// Keycodes of the modifier keys
   pub keys: HashSet<A>,
   /// `true` if the group is masking
   #[cfg_attr(feature = "serde", serde(default = "bool::default"))]
   pub masking: bool,
}

impl<A: Eq + Hash + Debug + Clone + Ord, Z: Eq + Hash + Debug + Clone + Ord> Config<A, Z> {
   /// Validate configuration. Returns:
   ///
   /// * `OK(warnings)` a list of warnings it the configuration is valid
   /// * `Err(error)` the first error if the configuration is invalid
   pub fn validate(&self) -> Result<Vec<ConfigValidationWarning<A>>, ConfigValidationError<A>> {
      let mut warnings = vec![];
      let mut ids: HashMap<_, Vec<_>> = HashMap::new();
      let mut modifier_keys: HashSet<A> = HashSet::new();
      let mut groups = HashSet::new();
      for modifier in &self.modifiers {
         if ids
            .insert(modifier.id.clone(), modifier.keys.iter().collect())
            .is_some()
         {
            Err(DuplicateModifierId(modifier.id.clone()))?;
         }
         let mut group: Vec<A> = modifier.keys.iter().cloned().collect();
         group.sort_unstable();
         if !groups.insert(group.clone()) {
            Err(DuplicateModifierGroup(group))?;
         }
         modifier_keys.extend(modifier.keys.iter().cloned());
         if modifier.keys.is_empty() {
            warnings.push(EmptyModifierGroup(modifier.id.clone()));
         }
      }

      let mut keys = HashSet::new();
      for action in &self.actions {
         if !keys.insert(action.key.clone()) {
            Err(DuplicateAction(action.key.clone()))?;
         }
         if action.immediate && !modifier_keys.contains(&action.key) {
            warnings.push(ImmediateAction(action.key.clone()));
         }
         let mut modifiers = HashSet::new();
         for combo in &action.modified {
            if let Some(group) = ids.get(&combo.modifier) {
               if group.contains(&&action.key) {
                  Err(SelfModifier(action.key.clone()))?;
               }
            } else {
               Err(UndefinedModifier(combo.modifier.clone(), action.key.clone()))?;
            }
            if !modifiers.insert(combo.modifier.clone()) {
               Err(DuplicateModifiedAction(combo.modifier.clone(), action.key.clone()))?;
            }
         }
      }
      Ok(warnings)
   }
}
