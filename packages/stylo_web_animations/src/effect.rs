use style::properties::OwnedPropertyDeclarationId;
use style::properties::animated_properties::{AnimationValue, AnimationValueMap};
use style::servo::animation_compose::{IterationComposite, compose};
use style::servo::animation_timing::{ComputedTiming, EffectTiming, Phase, before_flag};
use style::servo::animation_update::PropertySegments;
use style::values::computed::easing::TimingFunction;
use style::values::specified::animation::AnimationComposition;

/// A [keyframe effect] whose keyframes have been computed.
///
/// [keyframe effect]: https://drafts.csswg.org/web-animations-1/#keyframe-effects
#[derive(Clone, Debug)]
pub struct KeyframeEffect {
    pub timing: EffectTiming,
    /// The easing applied to the progress of each iteration.
    pub easing: TimingFunction,
    pub iteration_composite: IterationComposite,
    pub properties: Vec<PropertySegments>,
}

impl KeyframeEffect {
    pub fn computed_timing(&self, local_time: Option<f64>, playback_rate: f64) -> ComputedTiming {
        ComputedTiming::new(&self.timing, &self.easing, local_time, playback_rate)
    }

    /// Whether composing this effect can read the underlying value of a property.
    pub fn needs_underlying_values(&self) -> bool {
        self.properties.iter().any(|property| {
            property.segments.iter().any(|segment| {
                segment.from_value.is_none()
                    || segment.to_value.is_none()
                    || segment.from_composite != AnimationComposition::Replace
                    || segment.to_composite != AnimationComposition::Replace
            })
        }) || self.iteration_composite == IterationComposite::Accumulate
    }

    /// Composes this effect on top of `values`. Does nothing if the effect is not
    /// [in effect](https://drafts.csswg.org/web-animations-1/#in-effect).
    ///
    /// `base_value` returns the value of a property without animations.
    pub fn compose(
        &self,
        timing: &ComputedTiming,
        values: &mut AnimationValueMap,
        base_value: &mut dyn FnMut(&OwnedPropertyDeclarationId) -> Option<AnimationValue>,
    ) {
        let Some(progress) = timing.progress else {
            return;
        };
        let current_iteration = timing.current_iteration.unwrap_or(0.);
        let current_iteration = if current_iteration.is_finite() {
            current_iteration as u64
        } else {
            u64::MAX
        };
        for property in &self.properties {
            let Some(last_segment) = property.segments.last() else {
                continue;
            };
            compose(
                values,
                || base_value(&property.property),
                &property.property,
                property.segment_for_progress(progress),
                last_segment,
                self.iteration_composite,
                progress,
                current_iteration,
                before_flag(timing.phase, timing.current_direction),
            );
        }
    }
}

/// <https://drafts.csswg.org/web-animations-1/#current>
pub(crate) fn is_current(phase: Phase, playback_rate: f64) -> bool {
    match phase {
        Phase::Active => true,
        Phase::Before => playback_rate > 0.,
        Phase::After => playback_rate < 0.,
        Phase::Idle => false,
    }
}
