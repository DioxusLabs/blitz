//! The native half of the Web Animations bindings. The interfaces themselves
//! (`Animation`, `KeyframeEffect`, `DocumentTimeline`) are defined in
//! `animation.js` on top of the `__blitz_anim` function registered here.

use blitz_dom::BaseDocument;
use blitz_dom::web_animations::stylo_web_animations::{
    Action, Animation, AnimationId, Error, EventKind, PlayState,
};
use blitz_dom::web_animations::{Keyframe, KeyframeEffectOptions};
use boa_engine::object::builtins::JsArray;
use boa_engine::value::JsValue;
use boa_engine::{Context, JsNativeError, JsResult};
use style::selector_parser::PseudoElement;
use style::servo::animation_compose::IterationComposite;
use style::servo::animation_timing::EffectTiming;
use style::values::computed::easing::TimingFunction;
use style::values::specified::animation::AnimationComposition;
use style::values::specified::animation::{AnimationDirection, AnimationFillMode};

use crate::dom::{dom_ctx, dom_exception, js_str, node_id_of_value, node_wrapper, to_rust_string};

pub(crate) const ANIMATION_JS: &str = include_str!("animation.js");

fn time_value(time: Option<f64>) -> JsValue {
    time.map_or(JsValue::null(), JsValue::from)
}

fn time_arg(value: Option<&JsValue>) -> Option<f64> {
    value.and_then(|value| value.as_number())
}

fn element(value: &JsValue, index: u32, context: &mut Context) -> JsResult<JsValue> {
    match value.as_object() {
        Some(object) => object.get(index, context),
        None => Ok(JsValue::undefined()),
    }
}

fn length(value: &JsValue, context: &mut Context) -> JsResult<u32> {
    match value.as_object() {
        Some(object) => Ok(object
            .get(boa_engine::js_string!("length"), context)?
            .to_u32(context)?),
        None => Ok(0),
    }
}

fn parse_pseudo(pseudo: &str) -> Option<PseudoElement> {
    match pseudo {
        "::before" | ":before" => Some(PseudoElement::Before),
        "::after" | ":after" => Some(PseudoElement::After),
        _ => None,
    }
}

pub(crate) fn pseudo_name(pseudo: &PseudoElement) -> &'static str {
    match pseudo {
        PseudoElement::Before => "::before",
        PseudoElement::After => "::after",
        _ => "",
    }
}

fn composite(name: &str) -> AnimationComposition {
    match name {
        "add" => AnimationComposition::Add,
        "accumulate" => AnimationComposition::Accumulate,
        _ => AnimationComposition::Replace,
    }
}

/// Reads the timing array that `animation.js` builds.
fn effect_timing(
    doc: &BaseDocument,
    timing: &JsValue,
    context: &mut Context,
) -> JsResult<(EffectTiming, TimingFunction)> {
    let mut number =
        |index| -> JsResult<f64> { element(timing, index, context)?.to_number(context) };
    let delay = number(0)?;
    let end_delay = number(1)?;
    let iteration_start = number(3)?;
    let iterations = number(4)?;
    let duration = number(5)?;
    let mut string = |index| -> JsResult<String> {
        let value = element(timing, index, context)?;
        to_rust_string(&value, context)
    };
    let fill = match string(2)?.as_str() {
        "forwards" => AnimationFillMode::Forwards,
        "backwards" => AnimationFillMode::Backwards,
        "both" => AnimationFillMode::Both,
        _ => AnimationFillMode::None,
    };
    let direction = match string(6)?.as_str() {
        "reverse" => AnimationDirection::Reverse,
        "alternate" => AnimationDirection::Alternate,
        "alternate-reverse" => AnimationDirection::AlternateReverse,
        _ => AnimationDirection::Normal,
    };
    let easing = doc
        .parse_easing(&string(7)?)
        .unwrap_or_else(|| doc.parse_easing("linear").expect("linear is an easing"));
    let timing = EffectTiming {
        delay,
        end_delay,
        fill,
        iteration_start,
        iterations,
        duration,
        direction,
    };
    Ok((timing, easing))
}

/// Reads `[timing, keyframes]` as `animation.js` builds it.
fn effect_options(
    doc: &BaseDocument,
    spec: &JsValue,
    context: &mut Context,
) -> JsResult<KeyframeEffectOptions> {
    let timing_spec = element(spec, 0, context)?;
    let (timing, easing) = effect_timing(doc, &timing_spec, context)?;
    let linear = || doc.parse_easing("linear").expect("linear is an easing");
    let mut string = |index| -> JsResult<String> {
        let value = element(&timing_spec, index, context)?;
        to_rust_string(&value, context)
    };
    let effect_composite = composite(&string(8)?);
    let iteration_composite = match string(9)?.as_str() {
        "accumulate" => IterationComposite::Accumulate,
        _ => IterationComposite::Replace,
    };

    let keyframe_specs = element(spec, 1, context)?;
    let mut keyframes = Vec::new();
    for index in 0..length(&keyframe_specs, context)? {
        let keyframe = element(&keyframe_specs, index, context)?;
        let offset = element(&keyframe, 0, context)?.to_number(context)? as f32;
        let easing = element(&keyframe, 1, context)?;
        let easing = doc
            .parse_easing(&to_rust_string(&easing, context)?)
            .unwrap_or_else(linear);
        let keyframe_composite = element(&keyframe, 2, context)?;
        let keyframe_composite = match keyframe_composite.is_null_or_undefined() {
            true => None,
            false => Some(composite(&to_rust_string(&keyframe_composite, context)?)),
        };
        let properties = element(&keyframe, 3, context)?;
        let mut declarations = style::properties::PropertyDeclarationBlock::new();
        for index in (0..length(&properties, context)?).step_by(2) {
            let name = element(&properties, index, context)?;
            let name = to_rust_string(&name, context)?;
            let value = element(&properties, index + 1, context)?;
            let value = to_rust_string(&value, context)?;
            if let Some(block) = doc.parse_keyframe_declaration(&name, &value) {
                for declaration in block.declarations() {
                    declarations.push(
                        declaration.clone(),
                        style::properties::declaration_block::Importance::Normal,
                    );
                }
            }
        }
        keyframes.push(Keyframe {
            offset,
            easing,
            composite: keyframe_composite,
            declarations,
        });
    }

    Ok(KeyframeEffectOptions {
        timing,
        easing,
        composite: effect_composite,
        iteration_composite,
        keyframes,
    })
}

fn play_state(state: PlayState) -> &'static str {
    match state {
        PlayState::Idle => "idle",
        PlayState::Running => "running",
        PlayState::Paused => "paused",
        PlayState::Finished => "finished",
    }
}

fn error(context: &mut Context, error: Error) -> boa_engine::JsError {
    match error {
        Error::InvalidState => dom_exception(context, "InvalidStateError", "Invalid state"),
        Error::Type => JsNativeError::typ().with_message("Invalid time").into(),
    }
}

/// `__blitz_anim(op, ...args)`
pub(crate) fn animation_op(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let op = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let id = args
        .get(1)
        .and_then(|value| value.as_number())
        .map(|id| AnimationId::from_u64(id as u64));
    let arg = args.get(2);

    // Operations on the state machine of one animation
    type Update<'a> =
        Box<dyn FnOnce(&mut Animation, Option<f64>, &mut Vec<Action>) -> Result<(), Error> + 'a>;
    let update: Option<Update> = match op.as_str() {
        "play" => Some(Box::new(|a, t, actions| a.play(true, t, actions))),
        "pause" => Some(Box::new(|a, t, actions| a.pause(t, actions))),
        "finish" => Some(Box::new(|a, t, actions| a.finish(t, actions))),
        "reverse" => Some(Box::new(|a, t, actions| a.reverse(t, actions))),
        "cancel" => Some(Box::new(|a, t, actions| {
            a.cancel(t, actions);
            Ok(())
        })),
        "finishNotification" => Some(Box::new(|a, t, actions| {
            a.run_finish_notification(t, actions);
            Ok(())
        })),
        "setCurrentTime" => {
            let time = time_arg(arg);
            Some(Box::new(move |a, t, actions| {
                a.set_current_time(time, t, actions)
            }))
        }
        "setStartTime" => {
            let time = time_arg(arg);
            Some(Box::new(move |a, t, actions| {
                a.set_start_time(time, t, actions);
                Ok(())
            }))
        }
        "setPlaybackRate" => {
            let rate = time_arg(arg).unwrap_or(1.);
            Some(Box::new(move |a, t, actions| {
                a.set_playback_rate(rate, t, actions);
                Ok(())
            }))
        }
        "updatePlaybackRate" => {
            let rate = time_arg(arg).unwrap_or(1.);
            Some(Box::new(move |a, t, actions| {
                a.update_playback_rate(rate, t, actions);
                Ok(())
            }))
        }
        _ => None,
    };
    if let (Some(update), Some(id)) = (update, id) {
        let result = ctx
            .doc
            .borrow_mut()
            .animations_mut()
            .update_animation(id, update);
        return match result {
            Some(Err(e)) => Err(error(context, e)),
            _ => Ok(JsValue::undefined()),
        };
    }

    match (op.as_str(), id) {
        ("create", _) => {
            let mut doc = ctx.doc.borrow_mut();
            let id = doc.create_animation();
            doc.animations_mut().retain(id);
            Ok(JsValue::from(id.as_u64() as f64))
        }
        ("retain", Some(id)) => {
            ctx.doc.borrow_mut().animations_mut().retain(id);
            Ok(JsValue::undefined())
        }
        ("setEffect", Some(id)) => {
            let mut node = arg.and_then(node_id_of_value);
            let pseudo = match args.get(3) {
                Some(value) if !value.is_null_or_undefined() => {
                    let pseudo = parse_pseudo(&to_rust_string(value, context)?);
                    // An effect on a pseudo-element that is not supported must not animate
                    // the element itself.
                    if pseudo.is_none() {
                        node = None;
                    }
                    pseudo
                }
                _ => None,
            };
            let spec = args.get(4).cloned().unwrap_or_default();
            let options = match spec.is_null_or_undefined() {
                true => None,
                false => {
                    let doc = ctx.doc.borrow();
                    Some(effect_options(&doc, &spec, context)?)
                }
            };
            ctx.doc
                .borrow_mut()
                .set_animation_effect(id, node.map(|node| (node, pseudo)), options);
            Ok(JsValue::undefined())
        }
        ("stylesToCommit", Some(id)) => {
            use blitz_dom::web_animations::CommitStylesError;
            ctx.doc.borrow_mut().resolve(0.0);
            let declarations = ctx.doc.borrow().animation_styles_to_commit(id);
            match declarations {
                Ok(declarations) => {
                    let values = declarations
                        .into_iter()
                        .flat_map(|(name, value)| [js_str(&name), js_str(&value)]);
                    Ok(JsArray::from_iter(values, context).into())
                }
                Err(CommitStylesError::NoModificationAllowed) => {
                    Ok(js_str("NoModificationAllowedError"))
                }
                Err(CommitStylesError::NotRendered) => Ok(js_str("InvalidStateError")),
            }
        }
        ("replaceState", Some(id)) => {
            use blitz_dom::web_animations::stylo_web_animations::ReplaceState;
            Ok(js_str(
                match ctx.doc.borrow().animations().replace_state(id) {
                    ReplaceState::Active => "active",
                    ReplaceState::Removed => "removed",
                    ReplaceState::Persisted => "persisted",
                },
            ))
        }
        ("persist", Some(id)) => {
            ctx.doc.borrow_mut().persist_animation(id);
            Ok(JsValue::undefined())
        }
        ("setTimeline", Some(id)) => {
            let has_timeline = arg.is_some_and(|value| value.to_boolean());
            let mut doc = ctx.doc.borrow_mut();
            doc.animations_mut().set_has_timeline(id, has_timeline);
            Ok(JsValue::undefined())
        }
        ("state", Some(id)) => {
            let values = {
                let doc = ctx.doc.borrow();
                let store = doc.animations();
                let Some(animation) = store.animation(id) else {
                    return Ok(JsValue::null());
                };
                let time = store.timeline_time_of(id);
                [
                    time_value(animation.current_time(time)),
                    time_value(animation.start_time()),
                    JsValue::from(animation.playback_rate()),
                    js_str(play_state(animation.play_state(time))),
                    JsValue::from(animation.pending()),
                ]
            };
            Ok(JsArray::from_iter(values, context).into())
        }
        ("computedTiming", Some(id)) => {
            let values = {
                let doc = ctx.doc.borrow();
                let store = doc.animations();
                let (Some(timing), Some(effect)) = (store.computed_timing(id), store.effect(id))
                else {
                    return Ok(JsValue::null());
                };
                [
                    JsValue::from(effect.timing.active_duration()),
                    JsValue::from(effect.timing.end_time()),
                    time_value(timing.progress),
                    time_value(timing.current_iteration),
                ]
            };
            Ok(JsArray::from_iter(values, context).into())
        }
        ("effectTiming", Some(id)) => {
            use style_traits::ToCss;
            let values = {
                let doc = ctx.doc.borrow();
                let Some(effect) = doc.animations().effect(id) else {
                    return Ok(JsValue::null());
                };
                let timing = &effect.timing;
                [
                    JsValue::from(timing.delay),
                    JsValue::from(timing.end_delay),
                    js_str(match timing.fill {
                        AnimationFillMode::None => "none",
                        AnimationFillMode::Forwards => "forwards",
                        AnimationFillMode::Backwards => "backwards",
                        AnimationFillMode::Both => "both",
                    }),
                    JsValue::from(timing.iteration_start),
                    JsValue::from(timing.iterations),
                    JsValue::from(timing.duration),
                    js_str(match timing.direction {
                        AnimationDirection::Normal => "normal",
                        AnimationDirection::Reverse => "reverse",
                        AnimationDirection::Alternate => "alternate",
                        AnimationDirection::AlternateReverse => "alternate-reverse",
                    }),
                    js_str(&effect.easing.to_css_string()),
                ]
            };
            Ok(JsArray::from_iter(values, context).into())
        }
        ("setEffectTiming", Some(id)) => {
            let spec = arg.cloned().unwrap_or_default();
            let (timing, easing) = {
                let doc = ctx.doc.borrow();
                effect_timing(&doc, &spec, context)?
            };
            ctx.doc
                .borrow_mut()
                .set_animation_timing(id, timing, easing);
            Ok(JsValue::undefined())
        }
        ("timelineTime", _) => Ok(JsValue::from(ctx.doc.borrow().animations().timeline_time())),
        ("needsFrames", _) => Ok(JsValue::from(ctx.doc.borrow().animations().needs_ticks())),
        ("takeActions", _) => {
            let actions = ctx.doc.borrow_mut().animations_mut().take_actions();
            let mut result = Vec::with_capacity(actions.len());
            for (id, action) in actions {
                let (name, current_time, timeline_time) = match action {
                    Action::ReplaceReadyPromise => ("replaceReady", None, None),
                    Action::ResolveReadyPromise => ("resolveReady", None, None),
                    Action::AbortReadyPromise => ("abortReady", None, None),
                    Action::ReplaceFinishedPromise => ("replaceFinished", None, None),
                    Action::ResolveFinishedPromise => ("resolveFinished", None, None),
                    Action::AbortFinishedPromise => ("abortFinished", None, None),
                    Action::QueueFinishNotification => ("finishNotification", None, None),
                    Action::QueueEvent {
                        kind,
                        current_time,
                        timeline_time,
                    } => (
                        match kind {
                            EventKind::Finish => "finish",
                            EventKind::Cancel => "cancel",
                            EventKind::Remove => "remove",
                        },
                        current_time,
                        timeline_time,
                    ),
                };
                let values = [
                    JsValue::from(id.as_u64() as f64),
                    js_str(name),
                    time_value(current_time),
                    time_value(timeline_time),
                ];
                result.push(JsValue::from(JsArray::from_iter(values, context)));
            }
            Ok(JsArray::from_iter(result, context).into())
        }
        ("getAnimations", _) => {
            let root = arg.and_then(node_id_of_value);
            let subtree = args.get(3).is_some_and(|value| value.to_boolean());
            ctx.doc.borrow_mut().resolve(0.0);
            let animations: Vec<_> = {
                let doc = ctx.doc.borrow();
                doc.relevant_animations(root, subtree)
                    .into_iter()
                    .map(|id| {
                        let (kind, name) = match doc.css_animation_name(id) {
                            Some((false, name)) => ("animation", name),
                            Some((true, name)) => ("transition", name),
                            None => ("script", String::new()),
                        };
                        (id, kind, name, doc.animation_effect_target(id))
                    })
                    .collect()
            };
            let mut result = Vec::with_capacity(animations.len());
            for (id, kind, name, target) in animations {
                let (node, pseudo) = match target {
                    Some((node, pseudo)) => (
                        JsValue::from(node_wrapper(&ctx, node, context)),
                        pseudo.map_or(JsValue::null(), |pseudo| js_str(pseudo_name(&pseudo))),
                    ),
                    None => (JsValue::null(), JsValue::null()),
                };
                let values = [
                    JsValue::from(id.as_u64() as f64),
                    js_str(kind),
                    js_str(&name),
                    node,
                    pseudo,
                ];
                result.push(JsValue::from(JsArray::from_iter(values, context)));
            }
            Ok(JsArray::from_iter(result, context).into())
        }
        ("isEasing", _) => {
            let easing = to_rust_string(arg.unwrap_or(&JsValue::undefined()), context)?;
            Ok(JsValue::from(
                ctx.doc.borrow().parse_easing(&easing).is_some(),
            ))
        }
        ("isAnimatable", _) => {
            let property = to_rust_string(arg.unwrap_or(&JsValue::undefined()), context)?;
            Ok(JsValue::from(BaseDocument::is_animatable_property(
                &property,
            )))
        }
        ("isPseudo", _) => {
            let pseudo = to_rust_string(arg.unwrap_or(&JsValue::undefined()), context)?;
            Ok(JsValue::from(parse_pseudo(&pseudo).is_some()))
        }
        _ => Ok(JsValue::undefined()),
    }
}
