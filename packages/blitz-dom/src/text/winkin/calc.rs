//! `calc()` values that are not linear in their basis, which winkin resolves
//! through Stylo as layout learns the basis.
//!
//! A `calc()` whose percentages add up linearly, such as `calc(10px + 5%)`,
//! is passed on as a length and a fraction. Any other, such as
//! `max(20px, 10%)`, is cloned into the layout's [`CalcValues`] and passed on
//! as a [`CalcHandle`]: the address of its boxed clone, which [`StyloCalc`]
//! resolves. A layout keeps its clones until it is built again, so no handle
//! it holds outlives its value.

use std::sync::Mutex;

use style::values::computed::length_percentage::Unpacked;
use style::values::computed::{Length, LengthPercentage as StyloLengthPercentage, ToComputedValue};
use style::values::generics::calc::GenericCalcNode;
use style::values::specified::LengthPercentage as SpecifiedLengthPercentage;
use style::values::specified::calc::Leaf;
use style_traits::values::specified::AllowedNumericType;
use winkin::style::{CalcHandle, CalcResolver, LengthPercentage, LengthPercentageOrCalc};

/// A `calc()` and the device pixels per CSS pixel its layout is built at.
struct ScaledCalc {
    calc: StyloLengthPercentage,
    scale: f32,
}

/// The nonlinear `calc()` values a layout's styles hold, each boxed so that
/// its address, the handle winkin keeps, stays put.
#[derive(Default)]
pub(crate) struct CalcValues(Mutex<Vec<Box<ScaledCalc>>>);

impl CalcValues {
    /// Drops every value, for a new build. No handle to them is read after.
    pub(crate) fn clear(&mut self) {
        if let Ok(values) = self.0.get_mut() {
            values.clear();
        }
    }

    /// Returns `value` in device pixels: a length and a fraction where it is
    /// linear in its basis, and a handle to a clone of it otherwise.
    pub(crate) fn length_percentage(
        &self,
        value: &StyloLengthPercentage,
        scale: f32,
    ) -> LengthPercentageOrCalc {
        if is_linear(value) {
            return linear(value, scale).into();
        }
        let boxed = Box::new(ScaledCalc {
            calc: value.clone(),
            scale,
        });
        let handle = CalcHandle::new(&*boxed as *const ScaledCalc as usize as u64);
        match self.0.lock() {
            Ok(mut values) => {
                values.push(boxed);
                LengthPercentageOrCalc::Calc(handle)
            }
            // A poisoned store keeps nothing: take the value at a basis of
            // zero rather than hand out a handle nothing owns.
            Err(_) => LengthPercentageOrCalc::from_px(value.resolve(Length::new(0.0)).px() * scale),
        }
    }
}

/// Resolves the handles [`CalcValues`] hands out.
pub(crate) struct StyloCalc;

impl CalcResolver for StyloCalc {
    fn resolve(&self, calc: CalcHandle, basis: f32) -> f32 {
        // SAFETY: winkin resolves only the handles a layout's styles hold,
        // and the layout keeps their boxed values in its `CalcValues` until
        // it is built again, which replaces every handle.
        let value = unsafe { &*(calc.get() as usize as *const ScaledCalc) };
        let scale = value.scale;
        value.calc.resolve(Length::new(basis / scale)).px() * scale
    }
}

/// Returns `value` in device pixels as a length and a fraction, split by
/// resolving it against two bases: exact where it is linear in its basis.
pub(crate) fn linear(value: &StyloLengthPercentage, scale: f32) -> LengthPercentage {
    let at_zero = value.resolve(Length::new(0.0)).px();
    let at_hundred = value.resolve(Length::new(100.0)).px();
    LengthPercentage {
        px: at_zero * scale,
        fraction: (at_hundred - at_zero) / 100.0,
    }
}

/// Returns whether `value` is linear in its basis: a length, a percentage,
/// or a `calc()` that only adds, negates and scales them by constants.
///
/// A `calc()` its property clamps, such as padding's at zero, is not: the
/// clamp bends it, so splitting it by two bases would carry the clamp at one
/// of them to every basis.
pub(crate) fn is_linear(value: &StyloLengthPercentage) -> bool {
    if !matches!(value.unpack(), Unpacked::Calc(_)) {
        return true;
    }
    match SpecifiedLengthPercentage::from_computed_value(value) {
        SpecifiedLengthPercentage::Calc(calc) => {
            calc.0.clamping_mode == AllowedNumericType::All && node_is_linear(&calc.0.node)
        }
        _ => true,
    }
}

type Node = GenericCalcNode<Leaf>;

/// Returns whether `node` is linear in the basis.
fn node_is_linear(node: &Node) -> bool {
    if is_constant(node) {
        return true;
    }
    match node {
        Node::Leaf(_) => true,
        Node::Negate(child) => node_is_linear(child),
        Node::Sum(children) => children.iter().all(node_is_linear),
        Node::Product(factors) => {
            let mut varying = factors.iter().filter(|factor| !is_constant(factor));
            varying.next().is_some_and(node_is_linear) && varying.next().is_none()
        }
        _ => false,
    }
}

/// Returns whether `node` holds no percentage, so it is the same at every
/// basis.
fn is_constant(node: &Node) -> bool {
    let mut percentage = false;
    let mut node = node.clone();
    node.visit_depth_first(|node| {
        if let Node::Leaf(Leaf::Percentage(_)) = node {
            percentage = true;
        }
    });
    !percentage
}

#[cfg(test)]
mod tests {
    use style::typed_om::NumericBaseType;
    use style::values::computed::calc::CalcPercentageLeaf;
    use style::values::computed::length_percentage::{CalcNode, ComputedLeaf};
    use style::values::generics::Optional;
    use style::values::generics::calc::MinMaxOp;
    use style_traits::values::specified::AllowedNumericType;

    use super::*;

    fn px(px: f32) -> CalcNode {
        CalcNode::Leaf(ComputedLeaf::Length(Length::new(px)))
    }

    fn percent(fraction: f32) -> CalcNode {
        CalcNode::Leaf(ComputedLeaf::Percentage(CalcPercentageLeaf::new(
            fraction,
            Optional::Some(NumericBaseType::Length),
        )))
    }

    fn number(value: f32) -> CalcNode {
        CalcNode::Leaf(ComputedLeaf::Number(value))
    }

    fn calc(node: CalcNode, clamping: AllowedNumericType) -> StyloLengthPercentage {
        StyloLengthPercentage::new_calc(node, clamping)
    }

    /// Sums, negations and constant multiples of lengths and percentages are
    /// linear in the basis; `max()`, `min()` and `abs()` of a percentage are
    /// not, and neither is a sum its property clamps at zero.
    #[test]
    fn only_sums_and_scaled_percentages_are_linear() {
        let all = AllowedNumericType::All;
        let sum = CalcNode::Sum(vec![px(10.0), CalcNode::Negate(Box::new(percent(0.5)))].into());
        assert!(is_linear(&calc(sum, all)));
        let scaled = CalcNode::Sum(
            vec![
                px(1.0),
                CalcNode::Product(vec![percent(0.5), number(2.0)].into()),
            ]
            .into(),
        );
        assert!(is_linear(&calc(scaled, all)));
        let constant = CalcNode::Sum(
            vec![
                CalcNode::MinMax(vec![px(20.0), px(4.0)].into(), MinMaxOp::Max),
                percent(0.1),
            ]
            .into(),
        );
        assert!(is_linear(&calc(constant, all)));
        let max = CalcNode::MinMax(vec![px(20.0), percent(0.1)].into(), MinMaxOp::Max);
        assert!(!is_linear(&calc(max, all)));
        let min = CalcNode::MinMax(vec![px(20.0), percent(0.1)].into(), MinMaxOp::Min);
        assert!(!is_linear(&calc(min, all)));
        let clamped = CalcNode::Sum(vec![px(10.0), percent(-0.5)].into());
        assert!(!is_linear(&calc(clamped, AllowedNumericType::NonNegative)));
        let abs = CalcNode::Abs(Box::new(CalcNode::Sum(
            vec![px(10.0), percent(-0.5)].into(),
        )));
        assert!(!is_linear(&calc(abs, all)));
    }

    /// A nonlinear value becomes a handle that resolves as Stylo does, in
    /// device pixels: `max(20px, 10%)` at a scale of 2 is 40 device pixels
    /// against 200 and 80 against 800.
    #[test]
    fn a_handle_resolves_as_stylo_in_device_pixels() {
        let calcs = CalcValues::default();
        let max = CalcNode::MinMax(vec![px(20.0), percent(0.1)].into(), MinMaxOp::Max);
        let value = calc(max, AllowedNumericType::All);
        let LengthPercentageOrCalc::Calc(handle) = calcs.length_percentage(&value, 2.0) else {
            panic!("max() is not linear");
        };
        assert_eq!(StyloCalc.resolve(handle, 200.0), 40.0);
        assert_eq!(StyloCalc.resolve(handle, 800.0), 80.0);
        let linear = calc(
            CalcNode::Sum(vec![px(10.0), percent(0.05)].into()),
            AllowedNumericType::All,
        );
        let LengthPercentageOrCalc::LengthPercentage(split) = calcs.length_percentage(&linear, 2.0)
        else {
            panic!("a sum is linear");
        };
        assert_eq!((split.px, split.fraction), (20.0, 0.05));
    }
}
