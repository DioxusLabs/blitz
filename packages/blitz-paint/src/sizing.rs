use style::properties::generated::longhands::object_fit::computed_value::T as ObjectFit;

pub(crate) fn compute_object_fit(
    container_size: taffy::Size<f32>,
    object_size: Option<taffy::Size<f32>>,
    object_fit: ObjectFit,
) -> taffy::Size<f32> {
    match object_fit {
        ObjectFit::None => object_size.unwrap_or(container_size),
        ObjectFit::Fill => container_size,
        ObjectFit::Cover => compute_object_fit_cover(container_size, object_size),
        ObjectFit::Contain => compute_object_fit_contain(container_size, object_size),
        ObjectFit::ScaleDown => {
            let contain_size = compute_object_fit_contain(container_size, object_size);
            match object_size {
                Some(object_size) if object_size.width < contain_size.width => object_size,
                _ => contain_size,
            }
        }
    }
}

fn compute_object_fit_contain(
    container_size: taffy::Size<f32>,
    object_size: Option<taffy::Size<f32>>,
) -> taffy::Size<f32> {
    let Some(object_size) = object_size else {
        return container_size;
    };

    let x_ratio = container_size.width / object_size.width;
    let y_ratio = container_size.height / object_size.height;

    let ratio = match (x_ratio < 1.0, y_ratio < 1.0) {
        (true, true) => x_ratio.min(y_ratio),
        (true, false) => x_ratio,
        (false, true) => y_ratio,
        (false, false) => x_ratio.min(y_ratio),
    };

    object_size.map(|dim| dim * ratio)
}

fn compute_object_fit_cover(
    container_size: taffy::Size<f32>,
    object_size: Option<taffy::Size<f32>>,
) -> taffy::Size<f32> {
    let Some(object_size) = object_size else {
        return container_size;
    };

    let x_ratio = container_size.width / object_size.width;
    let y_ratio = container_size.height / object_size.height;

    let ratio = match (x_ratio < 1.0, y_ratio < 1.0) {
        (true, true) => x_ratio.max(y_ratio),
        (true, false) => y_ratio,
        (false, true) => x_ratio,
        (false, false) => x_ratio.max(y_ratio),
    };

    object_size.map(|dim| dim * ratio)
}
