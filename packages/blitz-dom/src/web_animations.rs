//! Integration of the [`stylo_web_animations`] animation store with the style traversal.
//!
//! The traversal only reads: each animated element gets its animation and transition rules
//! from [`WebAnimations::rules`] and queues an [`AnimationUpdate`] when its style changes
//! in a way that can affect animations. The updates are applied after the traversal.

use std::sync::Mutex;

use blitz_traits::node_id::NodeId;
use rustc_hash::FxHashMap;
use selectors::matching::QuirksMode;
use style::context::{
    CascadeInputs, SharedStyleContext, StyleContext, ThreadLocalStyleContext, UpdateAnimationsTasks,
};
use style::invalidation::element::restyle_hints::RestyleHint;
use style::properties::animated_properties::AnimationValue;
use style::properties::declaration_block::Importance;
use style::properties::{ComputedValues, OwnedPropertyDeclarationId, PropertyDeclarationBlock};
use style::properties::{
    PropertyDeclaration, PropertyId, SourcePropertyDeclaration, parse_one_declaration_into,
};
use style::rule_tree::RuleCascadeFlags;
use style::selector_parser::PseudoElement;
use style::servo::animation_compose::{IterationComposite, compute_keyframe_values};
use style::servo::animation_timing::EffectTiming;
use style::servo::animation_update::ComputedKeyframe;
use style::servo::animation_update::{
    build_property_segments, compute_css_keyframes, css_animations,
};
use style::servo_arc::Arc;
use style::shared_lock::{Locked, SharedRwLock};
use style::style_resolver::{
    PrimaryStyle, PseudoElementResolution, ResolvedStyle, StyleResolverForElement,
};
use style::stylesheets::{CssRuleType, Origin};
use style::stylist::RuleInclusion;
use style::values::computed::easing::TimingFunction;
use style::values::specified::animation::AnimationComposition;
use style_traits::ParsingMode;
use stylo_web_animations::{
    AnimationId, AnimationStore, ComposedValues, CssAnimation, KeyframeEffect, Target,
};

use style::global_style_data::GLOBAL_STYLE_DATA;
use style::shared_lock::StylesheetGuards;
use style::traversal_flags::TraversalFlags;

use crate::BaseDocument;
use crate::node::Node;
use crate::stylo::RegisteredPaintersImpl;

/// A request to update the animations of an element after the style traversal.
pub(crate) struct AnimationUpdate {
    pub node: NodeId,
    pub pseudo: Option<PseudoElement>,
    pub before_change_style: Option<Arc<ComputedValues>>,
    pub tasks: UpdateAnimationsTasks,
}

/// The output of the effect stack of a target, as rules for the cascade.
#[derive(Default)]
pub(crate) struct AnimationRules {
    composed: ComposedValues,
    /// Properties that animations stopped affecting at the last change of `composed`.
    previously_animated: Vec<OwnedPropertyDeclarationId>,
    pub animations: Option<Arc<Locked<PropertyDeclarationBlock>>>,
    pub transitions: Option<Arc<Locked<PropertyDeclarationBlock>>>,
}

impl AnimationRules {
    pub(crate) fn is_animated(&self, property: &OwnedPropertyDeclarationId) -> bool {
        self.composed.animations.contains_key(property)
            || self.previously_animated.contains(property)
    }
}

/// A keyframe of a keyframe effect that a script or the embedder created.
pub struct Keyframe {
    /// The [computed keyframe offset](https://drafts.csswg.org/web-animations-1/#computed-keyframe-offset).
    pub offset: f32,
    /// The easing from this keyframe to the next one.
    pub easing: TimingFunction,
    /// `None` uses the composite operation of the effect.
    pub composite: Option<AnimationComposition>,
    pub declarations: PropertyDeclarationBlock,
}

/// The parts of a keyframe effect other than its target.
pub struct KeyframeEffectOptions {
    pub timing: EffectTiming,
    /// The easing applied to the progress of each iteration.
    pub easing: TimingFunction,
    pub composite: AnimationComposition,
    pub iteration_composite: IterationComposite,
    /// Ordered by offset.
    pub keyframes: Vec<Keyframe>,
}

struct ScriptKeyframes {
    composite: AnimationComposition,
    keyframes: Vec<Keyframe>,
}

#[derive(Default)]
pub struct WebAnimations {
    pub(crate) store: AnimationStore,
    /// The keyframes of the effects that scripts or the embedder created. They are computed
    /// against the style of the target whenever it changes.
    script_keyframes: FxHashMap<AnimationId, ScriptKeyframes>,
    pub(crate) rules: FxHashMap<Target, AnimationRules>,
    /// The style without animations of the targets whose effects read underlying values.
    base_styles: FxHashMap<Target, Arc<ComputedValues>>,
    pub(crate) pending_updates: Mutex<Vec<AnimationUpdate>>,
}

pub(crate) fn animation_target(node: NodeId, pseudo: Option<PseudoElement>) -> Target {
    Target {
        node: node.as_u64() as usize,
        pseudo,
    }
}

fn target_node(target: &Target) -> NodeId {
    NodeId::from_u64(target.node as u64)
}

impl WebAnimations {
    /// Samples the effect stack of `target`. Returns whether its rules changed.
    fn compose(&mut self, target: &Target, guard: &SharedRwLock, is_tick: bool) -> bool {
        let base_style = self.base_styles.get(target);
        let composed = self.store.compose(target, &mut |property| {
            AnimationValue::from_computed_values(property.as_borrowed(), base_style?)
        });

        let rules = self.rules.entry(target.clone()).or_default();
        if is_tick {
            rules.previously_animated.clear();
        }
        if rules.composed == composed {
            if !self.store.has_animations(target) {
                self.rules.remove(target);
            }
            return false;
        }

        let rule = |values| {
            Arc::new(guard.wrap(PropertyDeclarationBlock::from_animation_value_map(values)))
        };
        rules.animations = (!composed.animations.is_empty()).then(|| rule(&composed.animations));
        rules.transitions = (!composed.transitions.is_empty()).then(|| rule(&composed.transitions));
        let previous = std::mem::replace(&mut rules.composed, composed);
        for property in previous.animations.into_keys() {
            if !rules.composed.animations.contains_key(&property) {
                rules.previously_animated.push(property);
            }
        }
        true
    }
}

fn restyle_hint(target: &Target) -> RestyleHint {
    match target.pseudo {
        Some(_) => RestyleHint::RESTYLE_SELF | RestyleHint::RESTYLE_PSEUDOS,
        None => RestyleHint::RESTYLE_SELF,
    }
}

/// The style of `target` without the rules of animations and transitions.
fn base_style<'a>(
    node: &'a Node,
    target: &Target,
    style: &Arc<ComputedValues>,
    context: &mut StyleContext<&'a Node>,
) -> Arc<ComputedValues> {
    let rules = style.rules();
    let base_rules = context
        .shared
        .stylist
        .rule_tree()
        .remove_animation_rules(rules);
    if base_rules == *rules {
        return style.clone();
    }
    let inputs = CascadeInputs {
        rules: Some(base_rules),
        visited_rules: style.visited_rules().cloned(),
        flags: style.flags.for_cascade_inputs(),
        included_cascade_flags: RuleCascadeFlags::empty(),
    };
    let mut resolver = StyleResolverForElement::new(
        node,
        context,
        RuleInclusion::All,
        PseudoElementResolution::IfApplicable,
    );
    let primary = match &target.pseudo {
        Some(_) => node.primary_styles().map(|style| style.clone()),
        None => None,
    };
    match (&target.pseudo, primary) {
        (Some(pseudo), Some(primary)) => {
            let primary = PrimaryStyle {
                style: ResolvedStyle(primary),
                reused_via_rule_node: false,
            };
            resolver
                .cascade_style_and_visited_for_pseudo_with_default_parents(inputs, pseudo, &primary)
                .0
        }
        _ => {
            resolver
                .cascade_style_and_visited_with_default_parents(inputs)
                .0
        }
    }
}

fn target_style(node: &Node, target: &Target) -> Option<Arc<ComputedValues>> {
    let data = node.try_stylo_element_data()?.get()?;
    match &target.pseudo {
        None => data.styles.get_primary().cloned(),
        Some(pseudo) => data.styles.pseudos.get(pseudo).cloned(),
    }
}

impl BaseDocument {
    /// Moves the document timeline to `now` (in milliseconds) and marks the elements whose
    /// animated style changed for restyling. Called before the style traversal.
    pub(crate) fn tick_animations(&mut self, now: f64) {
        if self.nodes.animations.store.is_empty() && self.nodes.animations.rules.is_empty() {
            return;
        }
        let mut animations = std::mem::take(&mut self.nodes.animations);

        // An element that was removed or lost its box is never restyled, so it would never
        // cancel its own animations.
        animations.store.retain_targets(|target| {
            self.nodes
                .get(target_node(target))
                .is_some_and(|node| node.flags.is_in_document() && node.primary_styles().is_some())
        });
        if now > animations.store.timeline_time() {
            animations.store.tick(now);
        }
        let store = &animations.store;
        animations
            .script_keyframes
            .retain(|id, _| store.animation(*id).is_some());

        let store = &animations.store;
        let mut targets: Vec<Target> = store.targets().cloned().collect();
        targets.extend(
            animations
                .rules
                .keys()
                .filter(|target| !store.has_animations(target))
                .cloned(),
        );
        animations
            .base_styles
            .retain(|target, _| store.needs_base_values(target));
        for target in targets {
            if animations.compose(&target, &self.guard, true) {
                if let Some(node) = self.nodes.get_mut(target_node(&target)) {
                    node.set_restyle_hint(restyle_hint(&target));
                }
            }
        }

        self.nodes.animations = animations;
    }

    /// Applies the animation updates that the style traversal queued. Returns whether
    /// elements need to be restyled as a result.
    pub(crate) fn update_animations(&mut self) -> bool {
        let updates = std::mem::take(self.nodes.animations.pending_updates.get_mut().unwrap());
        if updates.is_empty() {
            return false;
        }
        let mut animations = std::mem::take(&mut self.nodes.animations);
        let mut changed_targets = Vec::new();
        {
            let shared = &SharedStyleContext {
                traversal_flags: TraversalFlags::empty(),
                stylist: &self.stylist,
                options: GLOBAL_STYLE_DATA.options.clone(),
                guards: StylesheetGuards {
                    author: &self.guard.read(),
                    ua_or_user: &self.guard.read(),
                },
                visited_styles_enabled: false,
                animations: self.animations.clone(),
                current_time_for_animations: 0.,
                snapshot_map: &self.snapshots,
                registered_speculative_painters: &RegisteredPaintersImpl,
            };
            let mut thread_local = ThreadLocalStyleContext::new();
            let mut context = StyleContext {
                shared,
                thread_local: &mut thread_local,
            };
            let stylist = shared.stylist;
            for update in updates {
                let target = animation_target(update.node, update.pseudo);
                let Some(node) = self.nodes.get(update.node) else {
                    continue;
                };
                let Some(style) = target_style(node, &target) else {
                    animations.store.cancel_all(&target);
                    continue;
                };

                if update.tasks.contains(UpdateAnimationsTasks::CSS_ANIMATIONS) {
                    let specified = if style.get_box().clone_display().is_none() {
                        Vec::new()
                    } else {
                        let parent_style =
                            match &target.pseudo {
                                Some(_) => node.primary_styles().map(|style| style.clone()),
                                None => node.parent.and_then(|id| self.nodes.get(id)).and_then(
                                    |parent| parent.primary_styles().map(|style| style.clone()),
                                ),
                            };
                        css_animations(node, stylist, &style)
                            .map(|animation| {
                                let keyframes = compute_css_keyframes(
                                    node,
                                    target.pseudo.as_ref(),
                                    stylist,
                                    shared.guards.author,
                                    &style,
                                    parent_style.as_deref(),
                                    animation.keyframes,
                                    &animation.timing_function,
                                    animation.composition,
                                );
                                CssAnimation {
                                    name: animation.name,
                                    timing: animation.timing,
                                    paused: animation.paused,
                                    properties: build_property_segments(&keyframes),
                                }
                            })
                            .collect()
                    };
                    animations.store.update_css_animations(&target, specified);
                }

                if update
                    .tasks
                    .contains(UpdateAnimationsTasks::CSS_TRANSITIONS)
                {
                    if let Some(before_change_style) = &update.before_change_style {
                        let rules = animations.rules.get(&target);
                        animations.store.update_transitions(
                            &target,
                            before_change_style,
                            &style,
                            &|property| rules.is_some_and(|rules| rules.is_animated(property)),
                        );
                    }
                }

                let parent_style = match &target.pseudo {
                    Some(_) => node.primary_styles().map(|style| style.clone()),
                    None => node
                        .parent
                        .and_then(|id| self.nodes.get(id))
                        .and_then(|parent| parent.primary_styles().map(|style| style.clone())),
                };
                for id in animations.store.animations_of(&target).to_vec() {
                    let Some(script) = animations.script_keyframes.get(&id) else {
                        continue;
                    };
                    let keyframes: Vec<ComputedKeyframe> = script
                        .keyframes
                        .iter()
                        .map(|keyframe| ComputedKeyframe {
                            offset: keyframe.offset,
                            timing_function: Some(keyframe.easing.clone()),
                            composite: keyframe.composite.unwrap_or(script.composite),
                            values: compute_keyframe_values(
                                node,
                                target.pseudo.as_ref(),
                                stylist,
                                &style,
                                parent_style.as_deref(),
                                &keyframe.declarations,
                            ),
                        })
                        .collect();
                    animations
                        .store
                        .set_properties(id, build_property_segments(&keyframes));
                }

                if animations.store.needs_base_values(&target) {
                    let base_style = base_style(node, &target, &style, &mut context);
                    animations.base_styles.insert(target.clone(), base_style);
                }

                if animations.compose(&target, &self.guard, false) {
                    changed_targets.push(target);
                }
            }
        }

        for target in &changed_targets {
            if let Some(node) = self.nodes.get_mut(target_node(target)) {
                node.set_restyle_hint(restyle_hint(target));
            }
        }
        self.nodes.animations = animations;
        !changed_targets.is_empty()
    }
}

/// The API for animations that scripts or the embedder create.
impl BaseDocument {
    pub fn animations(&self) -> &AnimationStore {
        &self.nodes.animations.store
    }

    /// Changes made through this take effect when the document is next resolved.
    pub fn animations_mut(&mut self) -> &mut AnimationStore {
        &mut self.nodes.animations.store
    }

    /// Moves the document timeline forwards to `now` (in milliseconds) and updates every
    /// animation. Returns the new timeline time, which never decreases.
    pub fn advance_animation_timeline(&mut self, now: f64) -> f64 {
        let store = &mut self.nodes.animations.store;
        if now > store.timeline_time() {
            store.tick(now);
        }
        store.timeline_time()
    }

    /// Creates an idle animation without an effect.
    pub fn create_animation(&mut self) -> AnimationId {
        self.nodes.animations.store.create(None, None)
    }

    /// Creates an animation and plays it, as `Element.animate()` does.
    pub fn animate(
        &mut self,
        node: NodeId,
        pseudo: Option<PseudoElement>,
        effect: KeyframeEffectOptions,
    ) -> AnimationId {
        let id = self.create_animation();
        self.set_animation_effect(id, Some((node, pseudo)), Some(effect));
        self.nodes
            .animations
            .store
            .update_animation(id, |animation, time, actions| {
                let _ = animation.play(true, time, actions);
            });
        id
    }

    /// Replaces the effect of an animation and the target of that effect.
    pub fn set_animation_effect(
        &mut self,
        id: AnimationId,
        target: Option<(NodeId, Option<PseudoElement>)>,
        effect: Option<KeyframeEffectOptions>,
    ) {
        let animations = &mut self.nodes.animations;
        let old_target = animations.store.target(id).cloned();
        let target = target.map(|(node, pseudo)| animation_target(node, pseudo));
        match effect {
            Some(effect) => {
                animations.store.set_effect(
                    id,
                    Some(KeyframeEffect {
                        timing: effect.timing,
                        easing: effect.easing,
                        iteration_composite: effect.iteration_composite,
                        properties: Vec::new(),
                    }),
                );
                animations.script_keyframes.insert(
                    id,
                    ScriptKeyframes {
                        composite: effect.composite,
                        keyframes: effect.keyframes,
                    },
                );
            }
            None => {
                animations.store.set_effect(id, None);
                animations.script_keyframes.remove(&id);
            }
        }
        animations.store.set_target(id, target.clone());

        // The keyframes are computed when the target is restyled.
        for target in [old_target, target].into_iter().flatten() {
            if let Some(node) = self.nodes.get_mut(target_node(&target)) {
                node.set_restyle_hint(restyle_hint(&target));
            }
        }
    }

    /// Parses a property of a keyframe. Returns `None` if the property is not animatable or
    /// the value is invalid.
    pub fn parse_keyframe_declaration(
        &self,
        property: &str,
        value: &str,
    ) -> Option<PropertyDeclarationBlock> {
        let property_id = PropertyId::parse_enabled_for_all_content(property).ok()?;
        let mut source = SourcePropertyDeclaration::default();
        parse_one_declaration_into(
            &mut source,
            property_id,
            value,
            Origin::Author,
            &self.url.url_extra_data(),
            None,
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            CssRuleType::Keyframe,
        )
        .ok()?;
        let mut block = PropertyDeclarationBlock::new();
        block.extend(source.drain(), Importance::Normal);
        Some(block)
    }

    /// Parses an `<easing-function>`.
    pub fn parse_easing(&self, easing: &str) -> Option<TimingFunction> {
        let block = self.parse_keyframe_declaration("animation-timing-function", easing)?;
        match block.declarations() {
            [PropertyDeclaration::AnimationTimingFunction(list)] => match &*list.0 {
                [easing] => Some(easing.to_computed_value_without_context()),
                _ => None,
            },
            _ => None,
        }
    }
}
