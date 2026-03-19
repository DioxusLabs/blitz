use euclid::default::Rect;
use kurbo::{Affine, Vec2};
use style::{
    properties::generated::style_structs::Box as BoxStyleStruct,
    values::{
        computed::{CSSPixelLength, Rotate},
        generics::transform::{Scale, Translate},
    },
};

// 6. Current Transformation Matrix
//
// The transformation matrix is computed from the transform, transform-origin, translate, rotate, scale, and offset properties as follows:
//
//   - Start with the identity matrix.
//   - Translate by the computed X, Y, and Z values of transform-origin.
//   - Translate by the computed X, Y, and Z values of translate.
//   - Rotate by the computed <angle> about the specified axis of rotate.
//   - Scale by the computed X, Y, and Z values of scale.
//   - Translate and rotate by the transform specified by offset.
//   - Multiply by each of the transform functions in transform from left to right.
//   - Translate by the negated computed X, Y and Z values of transform-origin.
//
// <https://drafts.csswg.org/css-transforms-2/#ctm>
pub fn resolve_2d_transform(
    box_styles: &BoxStyleStruct,
    reference_box: Rect<CSSPixelLength>,
    scale: f64,
) -> Option<Affine> {
    let translate = match &box_styles.translate {
        Translate::None => None,
        Translate::Translate(x, y, _z) => Some(Vec2 {
            x: x.resolve(reference_box.width()).px() as f64,
            y: y.resolve(reference_box.height()).px() as f64,
        }),
    };

    let rotate = match &box_styles.rotate {
        Rotate::None => None,
        Rotate::Rotate(angle) => Some(angle.degrees() as f64),
        // TODO: support 3D transforms
        Rotate::Rotate3D(_, _, _, _) => None,
    };

    let scale_transform = match &box_styles.scale {
        Scale::None => None,
        Scale::Scale(x, y, _z) => Some(Vec2 {
            x: *x as f64 * scale,
            y: *y as f64 * scale,
        }),
    };

    let transform = if box_styles.transform.0.is_empty() {
        None
    } else {
        box_styles
            .transform
            .to_transform_3d_matrix(Some(&reference_box))
            .ok()
            .filter(|(_t, has_3d)| !has_3d)
            .map(|(t, _has_3d)| {
                // See: https://drafts.csswg.org/css-transforms-2/#two-dimensional-subset
                // And https://docs.rs/kurbo/latest/kurbo/struct.Affine.html#method.new
                Affine::new(
                    [
                        t.m11,
                        t.m12,
                        t.m21,
                        t.m22,
                        // Scale the translation but not the scale or skew
                        t.m41 * scale as f32,
                        t.m42 * scale as f32,
                    ]
                    .map(|v| v as f64),
                )
            })
    };

    // TODO: support the "offset" property
    // <https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/offset>

    if translate.is_none() && rotate.is_none() && scale_transform.is_none() && transform.is_none() {
        return None;
    }

    // Apply the transform origin by:
    //   - Translating by the origin offset
    //   - Applying our transform
    //   - Translating by the inverse of the origin offset
    let transform_origin = &box_styles.transform_origin;
    let origin_translation = Affine::translate(Vec2 {
        x: transform_origin
            .horizontal
            .resolve(reference_box.width())
            .px() as f64,
        y: transform_origin
            .vertical
            .resolve(reference_box.height())
            .px() as f64,
    });

    let mut resolved = Affine::IDENTITY; //Affine::translate(origin_translation);

    if let Some(translation) = translate {
        resolved = resolved.then_translate(translation)
    }

    if let Some(rotation) = rotate {
        resolved = resolved.then_rotate(rotation)
    }

    if let Some(scale_transform) = scale_transform {
        resolved = resolved.then_scale_non_uniform(scale_transform.x, scale_transform.y)
    }

    if let Some(transform) = transform {
        resolved *= transform;
    }

    resolved = origin_translation * resolved * origin_translation.inverse();

    if resolved != Affine::IDENTITY {
        Some(resolved)
    } else {
        None
    }
}
