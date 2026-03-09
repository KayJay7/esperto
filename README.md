## Esperto

This crate provides a performant implementation of the Esperto input system, a powerful and robust system for key combinations.
The implementation is generic, so that it can be easily plugged into new and existing systems, regardless of their needs.

The main functionalities are provided by the [`combo::ComboHandler`] struct.

The crate also provides a SDL3 based demo in the examples section, that prints recognized key combinations on a window.
Esperto input syste

### Definitions

Mapped keys are **action keys**. They can have a default mapped action and eventual *modified* actions.

A **modifier key** can modify, while pressed, some action keys (e.g. `alt`, `ctrl`). For this reason, modifiers keys generally perform their action at key-up instead of key-down (e.g. `win` activates at key-up, if no shortcuts have been performed).

Modifier keys are organized in *modifier groups* of one or more key (e.g. [`ctrl` `alt`]+`c`). Different groups can be used independently at the same time.

**combo**: a custom action for a *single* key pressed *while* a modifier group is down:

* example: [`ctrl` `alt`]+`c`
* it is **ordered**: the action key must `key-down` *after* the modifiers
    * modifier keys can `key-down` in any order: `ctrl`, `win`, `c` = `win`, `ctrl`, `c`
* it is **not time sensitive**

### Configuration example

```yaml
# Here we define modifier groups of one or more modifier key.
# Each group has a name
modifiers:
  - name: Ctrl
    keys: [ LCtrl ]
  - name: Alt
    keys: [ LAlt ]
  - name: CtrlAlt
    keys: [ LAlt, LCtrl ]
# Here we map actions to keys. The 'action' field can be omitted, to explicitly disable a key
actions:
  - key: M
    action: M
    modified:
        - modifier: Ctrl
          action: MediaPreviousTrack
        - modifier: Alt
          action: MediaNextTrack
        - modifier: CtrlAlt
          action: MediaStop
```

### Advanced topics

#### Options

* modifier group options
    * `masking = True`: While the modifier group is active, mask all actions which are not part of a combo.

      For example, `ctrl` + `I` usually doesn't produce `I` even if `I` has no explicit actions for `ctrl`.

      Note that at a given moment masking will encur if *any* masking modifier group is active.
* action options
    * `immediate = True`: Even if the key is a modifier key, activate immediately on key-down instead of waiting for key-up.
    * `latching = True`: When a modifier group deactivates, its currently active combos are deactivated as well, in accordance to OS behaviour. This option disables this behaviour, and the combo "latches" until the action key is released.

  For example, in a game where you have to keep pressed \[`shift`\]+`W` to run, this option would allow to release `shift` and keep the combo active until you release `W`.

#### Conflict resolution

Pressing a modifier key which activates a modifier group *G*, will cause subgroups of *G* to deactivate.

Pressing an action key *K* in presence of conflicts will prevent its combos from activating: overlapping/multiple active modifier groups for *K* will nullify, and only the default action will be performed (if any).

### Implementation

We implemented the above specification in a rust library available [here](). We focused on performance, by means of one-time precomputation and efficient memory layouts. We wrote it with InputPlumber in mind but it's quite generic (FYI, no vibe coding was employed).

The crate features an example SDL demo which shows the combos as seen by the system. Useful for debugging too.

---

### Future work: chords

The chord specification, summarized:

**chord**: a group of any keys, all pressed in a configurable interval *t*. Mostly useful in gaming:

* gamepad example: {`A` `B`}
* it is **unordered**: the time window dictates that the chord is valid, not the order
* it is **time sensitive**
    * the effect begins immediately when all key are down, if the first key-down happened within *t*
    * the effect of individual keys are delayed until the time window is over
        * when a key is relased, it fires key-down and key-up immediately
* a chord effect ends when any involved key is relased

In our architecture, the chord system sits in front of the combo system. It essentially acts as a temporal filter on the stream of events.\
It intrinsically introduces **delay**, which may vary depending on the existence of chords composed of keys within *t*, at any given time. In other words, if there are no chords which are supersets of the keys within *t*, the oldest key can immediately issued.

We are considering to expose an options to request a constant delay.

#### Implementation notes

An equivalent definition using a "time buffer":

* when a key is pressed, it enters the buffer. The buffer contains only pressed keys
* when a key reaches the end of the buffer, it is issued
* when a key is released, if the key is still in the buffer, it is removed and issued; then, key-up is issued
* when a key-down for button *b* reaches the end of the buffer, the **biggest and oldest** chord ⊆ buffer containing *b* is activated, and all its keys are removed from the buffer
* any key-up from an active chord key issues a key-up for the chord