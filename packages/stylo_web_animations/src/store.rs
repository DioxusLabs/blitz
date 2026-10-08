use rustc_hash::FxHashMap;
use style::Atom;
use style::properties::PropertyDeclarationIdSet;
use style::properties::animated_properties::{AnimationValue, AnimationValueMap};
use style::properties::{ComputedValues, OwnedPropertyDeclarationId};
use style::selector_parser::PseudoElement;
use style::servo::animation_compose::{AnimationPropertySegment, IterationComposite};
use style::servo::animation_timing::EffectTiming;
use style::servo::animation_update::{
    ExistingTransition, NewTransition, PropertySegments, RunningTransition, TransitionUpdate,
    TransitionUpdates, transition_updates,
};
use style::values::computed::easing::TimingFunction;
use style::values::computed::{AnimationDirection, AnimationFillMode};
use style::values::generics::easing::{BeforeFlag, TimingKeyword};
use style::values::specified::animation::AnimationComposition;

use crate::effect::is_current;
use crate::{Action, Animation, KeyframeEffect, PlayState};

/// The element or pseudo-element an effect applies to.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Target {
    /// The embedder's identifier for the element.
    pub node: usize,
    pub pseudo: Option<PseudoElement>,
}

/// Identifies an animation in an [`AnimationStore`]. Later animations have greater ids.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AnimationId(u64);

/// What created an animation.
#[derive(Clone, Debug)]
pub enum Origin {
    /// The embedder or a script.
    Script,
    /// The `animation-name` property.
    CssAnimation {
        name: Atom,
        /// The index in `animation-name`.
        index: usize,
    },
    /// The `transition-property` property.
    CssTransition {
        property: OwnedPropertyDeclarationId,
        end_value: AnimationValue,
        reversing_adjusted_start_value: AnimationValue,
        reversing_shortening_factor: f64,
    },
}

#[derive(Clone, Debug)]
struct Entry {
    animation: Animation,
    effect: KeyframeEffect,
    target: Target,
    origin: Origin,
}

#[derive(Clone, Debug, Default)]
struct TargetAnimations {
    /// In creation order.
    animations: Vec<AnimationId>,
    /// <https://drafts.csswg.org/css-transitions/#completed-transition>
    completed_transitions: Vec<(OwnedPropertyDeclarationId, AnimationValue)>,
}

/// A CSS animation that a style specifies, with its keyframes computed.
#[derive(Clone, Debug)]
pub struct CssAnimation {
    pub name: Atom,
    pub timing: EffectTiming,
    pub paused: bool,
    pub properties: Vec<PropertySegments>,
}

/// The values the animations of a target produce at each cascade level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComposedValues {
    /// For the animations cascade level.
    pub animations: AnimationValueMap,
    /// For the transitions cascade level.
    pub transitions: AnimationValueMap,
}

/// The animations of a document and its timeline.
#[derive(Debug)]
pub struct AnimationStore {
    /// The time of the document timeline, in milliseconds.
    timeline_time: f64,
    next_id: u64,
    animations: FxHashMap<AnimationId, Entry>,
    targets: FxHashMap<Target, TargetAnimations>,
    actions: Vec<(AnimationId, Action)>,
    scratch: Vec<Action>,
}

impl Default for AnimationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationStore {
    pub fn new() -> Self {
        Self {
            timeline_time: 0.,
            next_id: 0,
            animations: FxHashMap::default(),
            targets: FxHashMap::default(),
            actions: Vec::new(),
            scratch: Vec::new(),
        }
    }

    pub fn timeline_time(&self) -> f64 {
        self.timeline_time
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The targets that have animations or completed transitions.
    pub fn targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.keys()
    }

    pub fn has_animations(&self, target: &Target) -> bool {
        self.targets
            .get(target)
            .is_some_and(|target| !target.animations.is_empty())
    }

    pub fn has_css_animations(&self, target: &Target) -> bool {
        self.has_origin(target, |origin| {
            matches!(origin, Origin::CssAnimation { .. })
        })
    }

    pub fn has_css_transitions(&self, target: &Target) -> bool {
        self.has_origin(target, |origin| {
            matches!(origin, Origin::CssTransition { .. })
        })
    }

    fn has_origin(&self, target: &Target, matches: impl Fn(&Origin) -> bool) -> bool {
        self.targets.get(target).is_some_and(|target| {
            target
                .animations
                .iter()
                .any(|id| matches(&self.animations[id].origin))
        })
    }

    /// Whether the animations of `target` change as time passes.
    pub fn target_is_current(&self, target: &Target) -> bool {
        self.targets.get(target).is_some_and(|target| {
            target
                .animations
                .iter()
                .any(|id| self.entry_is_current(&self.animations[id]))
        })
    }

    /// Whether any animation changes as time passes.
    pub fn needs_ticks(&self) -> bool {
        self.animations
            .values()
            .any(|entry| self.entry_is_current(entry))
    }

    fn entry_is_current(&self, entry: &Entry) -> bool {
        let timeline_time = Some(self.timeline_time);
        let animation = &entry.animation;
        if animation.pending() {
            return true;
        }
        if animation.play_state(timeline_time) != PlayState::Running {
            return false;
        }
        let timing = entry.effect.computed_timing(
            animation.current_time(timeline_time),
            animation.playback_rate(),
        );
        is_current(timing.phase, animation.playback_rate())
    }

    pub fn animation(&self, id: AnimationId) -> Option<&Animation> {
        self.animations.get(&id).map(|entry| &entry.animation)
    }

    pub fn effect(&self, id: AnimationId) -> Option<&KeyframeEffect> {
        self.animations.get(&id).map(|entry| &entry.effect)
    }

    pub fn origin(&self, id: AnimationId) -> Option<&Origin> {
        self.animations.get(&id).map(|entry| &entry.origin)
    }

    pub fn target(&self, id: AnimationId) -> Option<&Target> {
        self.animations.get(&id).map(|entry| &entry.target)
    }

    /// The animations of `target` in creation order.
    pub fn animations_of(&self, target: &Target) -> &[AnimationId] {
        self.targets
            .get(target)
            .map_or(&[], |target| &target.animations)
    }

    /// Calls `update` with an animation, the timeline time and a list to add actions to.
    pub fn update_animation<R>(
        &mut self,
        id: AnimationId,
        update: impl FnOnce(&mut Animation, Option<f64>, &mut Vec<Action>) -> R,
    ) -> Option<R> {
        let entry = self.animations.get_mut(&id)?;
        let result = update(
            &mut entry.animation,
            Some(self.timeline_time),
            &mut self.scratch,
        );
        self.actions
            .extend(self.scratch.drain(..).map(|action| (id, action)));
        Some(result)
    }

    /// Replaces the effect of an animation.
    pub fn set_effect(&mut self, id: AnimationId, effect: KeyframeEffect) {
        let effect_end = effect.timing.end_time();
        if let Some(entry) = self.animations.get_mut(&id) {
            entry.effect = effect;
        }
        self.update_animation(id, |animation, time, actions| {
            animation.set_effect_end(effect_end, time, actions)
        });
    }

    /// The actions that animations have requested since the last call.
    pub fn take_actions(&mut self) -> Vec<(AnimationId, Action)> {
        std::mem::take(&mut self.actions)
    }

    fn insert(&mut self, target: Target, effect: KeyframeEffect, origin: Origin) -> AnimationId {
        let id = AnimationId(self.next_id);
        self.next_id += 1;
        self.targets
            .entry(target.clone())
            .or_default()
            .animations
            .push(id);
        self.animations.insert(
            id,
            Entry {
                animation: Animation::new(effect.timing.end_time()),
                effect,
                target,
                origin,
            },
        );
        id
    }

    /// Removes an animation without cancelling it.
    pub fn remove(&mut self, id: AnimationId) {
        let Some(entry) = self.animations.remove(&id) else {
            return;
        };
        if let Some(target) = self.targets.get_mut(&entry.target) {
            target.animations.retain(|other| *other != id);
            if target.animations.is_empty() && target.completed_transitions.is_empty() {
                self.targets.remove(&entry.target);
            }
        }
    }

    fn cancel_and_remove(&mut self, id: AnimationId) {
        self.update_animation(id, |animation, time, actions| {
            animation.cancel(time, actions)
        });
        self.remove(id);
    }

    /// Creates an animation for `effect` on `target` and plays it, as `Element.animate()`
    /// does. It becomes ready at the next [`AnimationStore::tick`].
    pub fn animate(&mut self, target: Target, effect: KeyframeEffect) -> AnimationId {
        let id = self.insert(target, effect, Origin::Script);
        self.update_animation(id, |animation, time, actions| {
            let _ = animation.play(true, time, actions);
        });
        id
    }

    /// Cancels and removes every animation of `target`.
    pub fn cancel_all(&mut self, target: &Target) {
        let Some(animations) = self.targets.remove(target) else {
            return;
        };
        for id in animations.animations {
            self.update_animation(id, |animation, time, actions| {
                animation.cancel(time, actions)
            });
            self.animations.remove(&id);
        }
    }

    /// Cancels and removes the animations of every target for which `keep` returns false.
    pub fn retain_targets(&mut self, mut keep: impl FnMut(&Target) -> bool) {
        let removed: Vec<Target> = self
            .targets
            .keys()
            .filter(|target| !keep(target))
            .cloned()
            .collect();
        for target in removed {
            self.cancel_all(&target);
        }
    }

    /// Moves the timeline to `timeline_time` and updates every animation.
    pub fn tick(&mut self, timeline_time: f64) {
        self.timeline_time = timeline_time;
        let mut finished_transitions = Vec::new();
        for (id, entry) in self.animations.iter_mut() {
            let time = Some(timeline_time);
            let animation = &mut entry.animation;
            animation.run_pending_task(timeline_time, &mut self.scratch);
            animation.tick(time, &mut self.scratch);
            animation.run_finish_notification(time, &mut self.scratch);
            self.actions
                .extend(self.scratch.drain(..).map(|action| (*id, action)));
            if matches!(entry.origin, Origin::CssTransition { .. })
                && animation.play_state(time) == PlayState::Finished
            {
                finished_transitions.push(*id);
            }
        }
        for id in finished_transitions {
            let entry = &self.animations[&id];
            if let Origin::CssTransition {
                property,
                end_value,
                ..
            } = &entry.origin
            {
                let completed = (property.clone(), end_value.clone());
                if let Some(target) = self.targets.get_mut(&entry.target) {
                    target.completed_transitions.push(completed);
                }
            }
            self.remove(id);
        }
    }

    /// Composes the effect stack of `target`.
    ///
    /// `base_value` returns the value of a property without animations.
    pub fn compose(
        &self,
        target: &Target,
        base_value: &mut dyn FnMut(&OwnedPropertyDeclarationId) -> Option<AnimationValue>,
    ) -> ComposedValues {
        let mut result = ComposedValues::default();
        let Some(target) = self.targets.get(target) else {
            return result;
        };

        // The composite order: CSS animations in the order of animation-name, then the other
        // animations in creation order. Transitions are on a cascade level of their own.
        let mut order: Vec<(usize, AnimationId)> = Vec::with_capacity(target.animations.len());
        for id in &target.animations {
            let entry = &self.animations[id];
            let (map, key) = match entry.origin {
                Origin::CssTransition { .. } => (&mut result.transitions, None),
                Origin::CssAnimation { index, .. } => (&mut result.animations, Some(index)),
                Origin::Script => (&mut result.animations, Some(usize::MAX)),
            };
            match key {
                Some(key) => order.push((key, *id)),
                None => self.compose_entry(entry, map, base_value),
            }
        }
        order.sort();
        for (_, id) in order {
            self.compose_entry(&self.animations[&id], &mut result.animations, base_value);
        }

        // A transition does not apply to a property that an animation overrides.
        if !result.animations.is_empty() {
            result
                .transitions
                .retain(|property, _| !result.animations.contains_key(property));
        }
        result
    }

    fn compose_entry(
        &self,
        entry: &Entry,
        values: &mut AnimationValueMap,
        base_value: &mut dyn FnMut(&OwnedPropertyDeclarationId) -> Option<AnimationValue>,
    ) {
        let animation = &entry.animation;
        let timing = entry.effect.computed_timing(
            animation.current_time(Some(self.timeline_time)),
            animation.playback_rate(),
        );
        entry.effect.compose(&timing, values, base_value);
    }

    /// Whether composing the animations of `target` can read values without animations.
    pub fn needs_base_values(&self, target: &Target) -> bool {
        self.targets.get(target).is_some_and(|target| {
            target
                .animations
                .iter()
                .any(|id| self.animations[id].effect.needs_underlying_values())
        })
    }

    /// Makes the CSS animations of `target` match `specified`, which is in the order of
    /// `animation-name`. Returns whether anything changed.
    ///
    /// <https://drafts.csswg.org/css-animations-2/#animations>
    pub fn update_css_animations(&mut self, target: &Target, specified: Vec<CssAnimation>) -> bool {
        let mut existing: Vec<(AnimationId, Atom)> = self
            .animations_of(target)
            .iter()
            .filter_map(|id| match &self.animations[id].origin {
                Origin::CssAnimation { name, .. } => Some((*id, name.clone())),
                _ => None,
            })
            .collect();
        let mut changed = false;

        for (index, specified) in specified.into_iter().enumerate() {
            let effect = KeyframeEffect {
                timing: specified.timing,
                easing: TimingFunction::Keyword(TimingKeyword::Linear),
                iteration_composite: IterationComposite::Replace,
                properties: specified.properties,
            };
            let origin = Origin::CssAnimation {
                name: specified.name.clone(),
                index,
            };
            let matching = existing
                .iter()
                .position(|(_, name)| *name == specified.name);
            let id = match matching {
                Some(position) => {
                    let (id, _) = existing.remove(position);
                    let was_paused = self.animations[&id]
                        .animation
                        .play_state(Some(self.timeline_time))
                        == PlayState::Paused;
                    self.animations.get_mut(&id).unwrap().origin = origin;
                    self.set_effect(id, effect);
                    if was_paused == specified.paused {
                        // The keyframes may still have changed.
                        changed = true;
                        continue;
                    }
                    id
                }
                None => self.insert(target.clone(), effect, origin),
            };
            changed = true;
            self.update_animation(id, |animation, time, actions| {
                let _ = if specified.paused {
                    animation.pause(time, actions)
                } else {
                    animation.play(false, time, actions)
                };
                // A style change is a point at which the animation is ready.
                if let Some(time) = time {
                    animation.run_pending_task(time, actions);
                }
            });
        }

        for (id, _) in existing {
            self.cancel_and_remove(id);
            changed = true;
        }
        changed
    }

    fn find_transition(
        &self,
        target: &Target,
        property: &OwnedPropertyDeclarationId,
    ) -> Option<AnimationId> {
        self.animations_of(target).iter().copied().find(|id| {
            matches!(&self.animations[id].origin,
                Origin::CssTransition { property: other, .. } if other == property)
        })
    }

    /// The transitions of `target` for properties that `transitioning` does not contain.
    fn stale_transitions(
        &self,
        target: &Target,
        transitioning: &PropertyDeclarationIdSet,
    ) -> impl Iterator<Item = AnimationId> {
        self.animations_of(target).iter().copied().filter(|id| {
            matches!(&self.animations[id].origin,
                Origin::CssTransition { property, .. }
                    if !transitioning.contains(property.as_borrowed()))
        })
    }

    fn transition_updates(
        &self,
        target: &Target,
        before_change_style: &ComputedValues,
        after_change_style: &ComputedValues,
        is_animated: &dyn Fn(&OwnedPropertyDeclarationId) -> bool,
    ) -> TransitionUpdates {
        let timeline_time = Some(self.timeline_time);
        let mut updates = transition_updates(before_change_style, after_change_style, |property| {
            if let Some(id) = self.find_transition(target, property) {
                let entry = &self.animations[&id];
                let Origin::CssTransition {
                    end_value,
                    reversing_adjusted_start_value,
                    reversing_shortening_factor,
                    ..
                } = &entry.origin
                else {
                    unreachable!()
                };
                let animation = &entry.animation;
                let timing = entry.effect.computed_timing(
                    animation.current_time(timeline_time),
                    animation.playback_rate(),
                );
                let segment = &entry.effect.properties[0].segments[0];
                let progress = timing.progress.unwrap_or(0.);
                let mut values = AnimationValueMap::default();
                entry.effect.compose(&timing, &mut values, &mut |_| None);
                let current_value = values
                    .remove(property)
                    .or_else(|| segment.from_value.clone())?;
                return Some(ExistingTransition {
                    end_value: end_value.clone(),
                    reversing_adjusted_start_value: reversing_adjusted_start_value.clone(),
                    reversing_shortening_factor: *reversing_shortening_factor,
                    running: Some(RunningTransition {
                        current_value,
                        timing_function_output: segment.position(progress, BeforeFlag::Unset),
                    }),
                });
            }
            let completed = &self.targets.get(target)?.completed_transitions;
            let (_, end_value) = completed.iter().find(|(other, _)| other == property)?;
            Some(ExistingTransition {
                end_value: end_value.clone(),
                reversing_adjusted_start_value: end_value.clone(),
                reversing_shortening_factor: 1.,
                running: None,
            })
        });

        updates.updates.retain(|update| {
            let property = match update {
                TransitionUpdate::Start(transition) => &transition.property,
                TransitionUpdate::Cancel(property)
                | TransitionUpdate::RemoveCompleted(property) => property,
            };
            !is_animated(property)
        });
        updates
    }

    /// Whether [`AnimationStore::update_transitions`] would change anything.
    pub fn needs_transitions_update(
        &self,
        target: &Target,
        before_change_style: &ComputedValues,
        after_change_style: &ComputedValues,
        is_animated: &dyn Fn(&OwnedPropertyDeclarationId) -> bool,
    ) -> bool {
        let updates =
            self.transition_updates(target, before_change_style, after_change_style, is_animated);
        let transitioning = &updates.transitioning_properties;
        !updates.updates.is_empty()
            || self
                .stale_transitions(target, transitioning)
                .next()
                .is_some()
            || self.targets.get(target).is_some_and(|target| {
                target
                    .completed_transitions
                    .iter()
                    .any(|(property, _)| !transitioning.contains(property.as_borrowed()))
            })
    }

    /// Starts and cancels the transitions of `target` for a style change. Returns whether
    /// anything changed.
    ///
    /// `is_animated` returns whether animations changed a property between the two styles.
    /// Those changes do not start or cancel transitions.
    ///
    /// <https://drafts.csswg.org/css-transitions/#starting>
    pub fn update_transitions(
        &mut self,
        target: &Target,
        before_change_style: &ComputedValues,
        after_change_style: &ComputedValues,
        is_animated: &dyn Fn(&OwnedPropertyDeclarationId) -> bool,
    ) -> bool {
        let updates =
            self.transition_updates(target, before_change_style, after_change_style, is_animated);

        let mut changed = false;
        for update in updates.updates {
            let property = match &update {
                TransitionUpdate::Start(transition) => &transition.property,
                TransitionUpdate::Cancel(property)
                | TransitionUpdate::RemoveCompleted(property) => property,
            };
            changed = true;
            if let Some(id) = self.find_transition(target, property) {
                self.cancel_and_remove(id);
            }
            self.remove_completed_transitions(target, |other| other != property);
            if let TransitionUpdate::Start(transition) = update {
                self.start_transition(target, transition);
            }
        }

        // Transitions for properties that no longer match transition-property.
        let transitioning = &updates.transitioning_properties;
        let stale: Vec<AnimationId> = self.stale_transitions(target, transitioning).collect();
        for id in stale {
            self.cancel_and_remove(id);
            changed = true;
        }
        self.remove_completed_transitions(target, |property| {
            transitioning.contains(property.as_borrowed())
        });
        changed
    }

    fn remove_completed_transitions(
        &mut self,
        target: &Target,
        mut keep: impl FnMut(&OwnedPropertyDeclarationId) -> bool,
    ) {
        let Some(animations) = self.targets.get_mut(target) else {
            return;
        };
        animations
            .completed_transitions
            .retain(|(property, _)| keep(property));
        if animations.animations.is_empty() && animations.completed_transitions.is_empty() {
            self.targets.remove(target);
        }
    }

    fn start_transition(&mut self, target: &Target, transition: NewTransition) {
        let effect = KeyframeEffect {
            timing: EffectTiming {
                delay: transition.delay,
                end_delay: 0.,
                fill: AnimationFillMode::Backwards,
                iteration_start: 0.,
                iterations: 1.,
                duration: transition.duration,
                direction: AnimationDirection::Normal,
            },
            easing: TimingFunction::Keyword(TimingKeyword::Linear),
            iteration_composite: IterationComposite::Replace,
            properties: vec![PropertySegments {
                property: transition.property.clone(),
                segments: vec![AnimationPropertySegment {
                    from_key: 0.,
                    to_key: 1.,
                    from_value: Some(transition.start_value),
                    to_value: Some(transition.end_value.clone()),
                    timing_function: Some(transition.timing_function),
                    from_composite: AnimationComposition::Replace,
                    to_composite: AnimationComposition::Replace,
                }],
            }],
        };
        let origin = Origin::CssTransition {
            property: transition.property,
            end_value: transition.end_value,
            reversing_adjusted_start_value: transition.reversing_adjusted_start_value,
            reversing_shortening_factor: transition.reversing_shortening_factor,
        };
        let id = self.insert(target.clone(), effect, origin);
        self.update_animation(id, |animation, time, actions| {
            let _ = animation.play(false, time, actions);
            // The start time of a transition is the time of the style change event.
            if let Some(time) = time {
                animation.run_pending_task(time, actions);
            }
        });
    }
}
