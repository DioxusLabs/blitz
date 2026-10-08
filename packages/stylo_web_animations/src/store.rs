use rustc_hash::FxHashMap;
use style::Atom;
use style::properties::PropertyDeclarationIdSet;
use style::properties::animated_properties::{AnimationValue, AnimationValueMap};
use style::properties::{ComputedValues, OwnedPropertyDeclarationId};
use style::selector_parser::PseudoElement;
use style::servo::animation_compose::{AnimationPropertySegment, IterationComposite};
use style::servo::animation_timing::{ComputedTiming, EffectTiming};
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

impl AnimationId {
    pub fn from_u64(id: u64) -> Self {
        Self(id)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

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
    effect: Option<KeyframeEffect>,
    target: Option<Target>,
    origin: Origin,
    /// Whether the animation is associated with the document timeline.
    has_timeline: bool,
    /// Whether style still creates and cancels this CSS animation or transition.
    owned_by_style: bool,
    /// The number of handles the embedder holds. An animation without handles is removed
    /// once it is no longer relevant.
    handles: u32,
}

impl Entry {
    fn timeline_time(&self, timeline_time: f64) -> Option<f64> {
        self.has_timeline.then_some(timeline_time)
    }

    fn computed_timing(&self, timeline_time: f64) -> Option<ComputedTiming> {
        let effect = self.effect.as_ref()?;
        let time = self.timeline_time(timeline_time);
        Some(effect.computed_timing(
            self.animation.current_time(time),
            self.animation.playback_rate(),
        ))
    }

    /// <https://drafts.csswg.org/web-animations-1/#relevant-animations>
    fn is_relevant(&self, timeline_time: f64) -> bool {
        self.computed_timing(timeline_time).is_some_and(|timing| {
            timing.progress.is_some() || is_current(timing.phase, self.animation.playback_rate())
        })
    }
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
            target.animations.iter().any(|id| {
                let entry = &self.animations[id];
                entry.owned_by_style && matches(&entry.origin)
            })
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
        let timeline_time = entry.timeline_time(self.timeline_time);
        let animation = &entry.animation;
        if animation.pending() {
            return true;
        }
        if animation.play_state(timeline_time) != PlayState::Running {
            return false;
        }
        entry
            .computed_timing(self.timeline_time)
            .is_some_and(|timing| is_current(timing.phase, animation.playback_rate()))
    }

    pub fn animation(&self, id: AnimationId) -> Option<&Animation> {
        self.animations.get(&id).map(|entry| &entry.animation)
    }

    pub fn effect(&self, id: AnimationId) -> Option<&KeyframeEffect> {
        self.animations.get(&id)?.effect.as_ref()
    }

    pub fn origin(&self, id: AnimationId) -> Option<&Origin> {
        self.animations.get(&id).map(|entry| &entry.origin)
    }

    pub fn target(&self, id: AnimationId) -> Option<&Target> {
        self.animations.get(&id)?.target.as_ref()
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
        let time = entry.timeline_time(self.timeline_time);
        let result = update(&mut entry.animation, time, &mut self.scratch);
        self.actions
            .extend(self.scratch.drain(..).map(|action| (id, action)));
        Some(result)
    }

    /// Replaces the effect of an animation.
    pub fn set_effect(&mut self, id: AnimationId, effect: Option<KeyframeEffect>) {
        let effect_end = effect
            .as_ref()
            .map_or(0., |effect| effect.timing.end_time());
        if let Some(entry) = self.animations.get_mut(&id) {
            entry.effect = effect;
        }
        self.update_animation(id, |animation, time, actions| {
            animation.set_effect_end(effect_end, time, actions)
        });
    }

    /// Replaces the computed keyframes of the effect of an animation.
    pub fn set_properties(&mut self, id: AnimationId, properties: Vec<PropertySegments>) {
        if let Some(effect) = self
            .animations
            .get_mut(&id)
            .and_then(|entry| entry.effect.as_mut())
        {
            effect.properties = properties;
        }
    }

    /// Changes the target of the effect of an animation.
    pub fn set_target(&mut self, id: AnimationId, target: Option<Target>) {
        let Some(entry) = self.animations.get_mut(&id) else {
            return;
        };
        if entry.target == target {
            return;
        }
        // A CSS animation or transition that a script retargets no longer belongs to the
        // style of its original target.
        entry.owned_by_style = false;
        let old_target = std::mem::replace(&mut entry.target, target.clone());
        if let Some(old_target) = old_target {
            self.unlink(id, &old_target);
        }
        if let Some(target) = target {
            let animations = &mut self.targets.entry(target).or_default().animations;
            let position = animations.partition_point(|other| *other < id);
            animations.insert(position, id);
        }
    }

    /// Associates an animation with the document timeline or with no timeline.
    pub fn set_has_timeline(&mut self, id: AnimationId, has_timeline: bool) {
        if let Some(entry) = self.animations.get_mut(&id) {
            entry.has_timeline = has_timeline;
        }
    }

    pub fn has_timeline(&self, id: AnimationId) -> bool {
        self.animations
            .get(&id)
            .is_some_and(|entry| entry.has_timeline)
    }

    /// The timeline time as the animation sees it.
    pub fn timeline_time_of(&self, id: AnimationId) -> Option<f64> {
        self.animations.get(&id)?.timeline_time(self.timeline_time)
    }

    /// Keeps an animation in the store until the matching [`AnimationStore::release`].
    pub fn retain(&mut self, id: AnimationId) {
        if let Some(entry) = self.animations.get_mut(&id) {
            entry.handles += 1;
        }
    }

    pub fn release(&mut self, id: AnimationId) {
        if let Some(entry) = self.animations.get_mut(&id) {
            entry.handles = entry.handles.saturating_sub(1);
        }
    }

    /// Whether an embedder handle keeps the animation in the store.
    pub fn is_retained(&self, id: AnimationId) -> bool {
        self.animations
            .get(&id)
            .is_some_and(|entry| entry.handles > 0)
    }

    /// The timing of the effect of an animation at the current time.
    pub fn computed_timing(&self, id: AnimationId) -> Option<ComputedTiming> {
        self.animations
            .get(&id)?
            .computed_timing(self.timeline_time)
    }

    /// Whether the animation is [relevant] or has a pending task.
    ///
    /// [relevant]: https://drafts.csswg.org/web-animations-1/#relevant-animations
    pub fn is_relevant(&self, id: AnimationId) -> bool {
        self.animations
            .get(&id)
            .is_some_and(|entry| entry.animation.pending() || entry.is_relevant(self.timeline_time))
    }

    /// The relevant animations of `target`, or of every target, in composite order.
    ///
    /// <https://drafts.csswg.org/web-animations-1/#animation-composite-order>
    pub fn relevant_animations(&self, target: Option<&Target>) -> Vec<AnimationId> {
        let mut result: Vec<AnimationId> = match target {
            Some(target) => self.animations_of(target).to_vec(),
            None => self
                .animations
                .iter()
                .filter(|(_, entry)| entry.target.is_some())
                .map(|(id, _)| *id)
                .collect(),
        };
        result.retain(|id| self.animations[id].is_relevant(self.timeline_time));
        result.sort_by_key(|id| {
            let entry = &self.animations[id];
            match (&entry.origin, entry.owned_by_style) {
                (Origin::CssTransition { .. }, true) => (0, 0, *id),
                (Origin::CssAnimation { index, .. }, true) => (1, *index, *id),
                _ => (2, 0, *id),
            }
        });
        result
    }

    /// The actions that animations have requested since the last call.
    pub fn take_actions(&mut self) -> Vec<(AnimationId, Action)> {
        std::mem::take(&mut self.actions)
    }

    fn insert(
        &mut self,
        target: Option<Target>,
        effect: Option<KeyframeEffect>,
        origin: Origin,
    ) -> AnimationId {
        let id = AnimationId(self.next_id);
        self.next_id += 1;
        if let Some(target) = &target {
            self.targets
                .entry(target.clone())
                .or_default()
                .animations
                .push(id);
        }
        let effect_end = effect
            .as_ref()
            .map_or(0., |effect| effect.timing.end_time());
        self.animations.insert(
            id,
            Entry {
                animation: Animation::new(effect_end),
                owned_by_style: !matches!(origin, Origin::Script),
                effect,
                target,
                origin,
                has_timeline: true,
                handles: 0,
            },
        );
        id
    }

    fn unlink(&mut self, id: AnimationId, target: &Target) {
        if let Some(animations) = self.targets.get_mut(target) {
            animations.animations.retain(|other| *other != id);
            if animations.animations.is_empty() && animations.completed_transitions.is_empty() {
                self.targets.remove(target);
            }
        }
    }

    /// Removes an animation without cancelling it.
    pub fn remove(&mut self, id: AnimationId) {
        let Some(entry) = self.animations.remove(&id) else {
            return;
        };
        if let Some(target) = &entry.target {
            self.unlink(id, target);
        }
    }

    /// Cancels an animation that style no longer specifies. It stays in the store as an
    /// ordinary animation if the embedder holds a handle to it.
    fn cancel_and_remove(&mut self, id: AnimationId) {
        self.update_animation(id, |animation, time, actions| {
            animation.cancel(time, actions)
        });
        match self.animations.get_mut(&id) {
            Some(entry) if entry.handles > 0 => entry.owned_by_style = false,
            _ => self.remove(id),
        }
    }

    /// Creates an idle animation.
    pub fn create(
        &mut self,
        target: Option<Target>,
        effect: Option<KeyframeEffect>,
    ) -> AnimationId {
        self.insert(target, effect, Origin::Script)
    }

    /// Creates an animation for `effect` on `target` and plays it, as `Element.animate()`
    /// does. It becomes ready at the next [`AnimationStore::tick`].
    pub fn animate(&mut self, target: Target, effect: KeyframeEffect) -> AnimationId {
        let id = self.insert(Some(target), Some(effect), Origin::Script);
        self.update_animation(id, |animation, time, actions| {
            let _ = animation.play(true, time, actions);
        });
        id
    }

    /// Cancels the CSS animations and transitions of `target`, and forgets its completed
    /// transitions.
    pub fn cancel_all(&mut self, target: &Target) {
        let owned_by_style: Vec<AnimationId> = self
            .animations_of(target)
            .iter()
            .copied()
            .filter(|id| self.animations[id].owned_by_style)
            .collect();
        for id in owned_by_style {
            self.cancel_and_remove(id);
        }
        self.remove_completed_transitions(target, |_| false);
    }

    /// Calls [`AnimationStore::cancel_all`] for every target for which `keep` returns false.
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
        let mut unused = Vec::new();
        let mut ids: Vec<AnimationId> = self.animations.keys().copied().collect();
        ids.sort();
        for id in ids {
            let entry = self.animations.get_mut(&id).unwrap();
            let time = entry.timeline_time(timeline_time);
            let animation = &mut entry.animation;
            if time.is_some() {
                animation.run_pending_task(timeline_time, &mut self.scratch);
            }
            animation.tick(time, &mut self.scratch);
            animation.run_finish_notification(time, &mut self.scratch);
            self.actions
                .extend(self.scratch.drain(..).map(|action| (id, action)));
            if matches!(entry.origin, Origin::CssTransition { .. })
                && entry.owned_by_style
                && animation.play_state(time) == PlayState::Finished
            {
                finished_transitions.push(id);
            } else if entry.handles == 0
                && !entry.owned_by_style
                && !animation.pending()
                && !entry.is_relevant(timeline_time)
            {
                unused.push(id);
            }
        }
        for id in finished_transitions {
            let entry = &self.animations[&id];
            if let (
                Origin::CssTransition {
                    property,
                    end_value,
                    ..
                },
                Some(target),
            ) = (&entry.origin, &entry.target)
            {
                let completed = (property.clone(), end_value.clone());
                if let Some(target) = self.targets.get_mut(target) {
                    target.completed_transitions.push(completed);
                }
            }
            match self.animations.get_mut(&id) {
                Some(entry) if entry.handles > 0 => entry.owned_by_style = false,
                _ => self.remove(id),
            }
        }
        for id in unused {
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
            let (map, key) = match (&entry.origin, entry.owned_by_style) {
                (Origin::CssTransition { .. }, true) => (&mut result.transitions, None),
                (Origin::CssAnimation { index, .. }, true) => {
                    (&mut result.animations, Some(*index))
                }
                _ => (&mut result.animations, Some(usize::MAX)),
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
        if let (Some(effect), Some(timing)) =
            (&entry.effect, entry.computed_timing(self.timeline_time))
        {
            effect.compose(&timing, values, base_value);
        }
    }

    /// Whether composing the animations of `target` can read values without animations.
    pub fn needs_base_values(&self, target: &Target) -> bool {
        self.targets.get(target).is_some_and(|target| {
            target.animations.iter().any(|id| {
                self.animations[id]
                    .effect
                    .as_ref()
                    .is_some_and(|effect| effect.needs_underlying_values())
            })
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
            .filter_map(|id| match &self.animations[id] {
                Entry {
                    origin: Origin::CssAnimation { name, .. },
                    owned_by_style: true,
                    ..
                } => Some((*id, name.clone())),
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
                    let entry = &self.animations[&id];
                    let was_paused = entry
                        .animation
                        .play_state(entry.timeline_time(self.timeline_time))
                        == PlayState::Paused;
                    self.animations.get_mut(&id).unwrap().origin = origin;
                    self.set_effect(id, Some(effect));
                    if was_paused == specified.paused {
                        // The keyframes may still have changed.
                        changed = true;
                        continue;
                    }
                    id
                }
                None => self.insert(Some(target.clone()), Some(effect), origin),
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
            let entry = &self.animations[id];
            entry.owned_by_style
                && matches!(&entry.origin,
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
            let entry = &self.animations[id];
            entry.owned_by_style
                && matches!(&entry.origin,
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
                let effect = entry.effect.as_ref()?;
                let timing = entry.computed_timing(self.timeline_time)?;
                let segment = effect
                    .properties
                    .iter()
                    .find(|segments| segments.property == *property)?
                    .segments
                    .first()?;
                let progress = timing.progress.unwrap_or(0.);
                let mut values = AnimationValueMap::default();
                effect.compose(&timing, &mut values, &mut |_| None);
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
        let id = self.insert(Some(target.clone()), Some(effect), origin);
        self.update_animation(id, |animation, time, actions| {
            let _ = animation.play(false, time, actions);
            // The start time of a transition is the time of the style change event.
            if let Some(time) = time {
                animation.run_pending_task(time, actions);
            }
        });
    }
}
