use anyrender::{Filter, PaintScene};
use kurbo::{Affine, Shape};
use peniko::Mix;
use std::sync::Arc;

pub(crate) struct LayerManager;

impl LayerManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_with_layer<S: PaintScene, F: FnOnce(&mut S)>(
        &self,
        scene: &mut S,
        condition: bool,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
        paint_layer: F,
    ) {
        let layer_used = self.maybe_push_layer(
            scene,
            condition,
            opacity,
            transform,
            shape,
            filter,
            backdrop_filter,
        );
        paint_layer(scene);
        self.maybe_pop_layer(scene, layer_used);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_push_layer(
        &self,
        scene: &mut impl PaintScene,
        condition: bool,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) -> bool {
        if !condition {
            return false;
        }

        if opacity == 1.0 && filter.is_none() && backdrop_filter.is_none() {
            scene.push_clip_layer(transform, shape);
        } else {
            scene.push_layer(
                Mix::Normal,
                opacity,
                transform,
                shape,
                filter,
                backdrop_filter,
            );
        };

        true
    }

    pub(crate) fn maybe_pop_layer(&self, scene: &mut impl PaintScene, condition: bool) {
        if condition {
            scene.pop_layer();
        }
    }
}

#[cfg(test)]
mod tests {
    use anyrender::{Scene, recording::RenderCommand};
    use kurbo::Rect;

    use super::*;

    #[test]
    fn does_not_drop_layers_after_1024() {
        const LAYER_COUNT: usize = 1200;

        let manager = LayerManager;
        let mut scene = Scene::new();
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);

        for index in 0..LAYER_COUNT {
            let opacity = if index % 2 == 0 { 1.0 } else { 0.5 };
            manager.maybe_with_layer(
                &mut scene,
                true,
                opacity,
                Affine::IDENTITY,
                &clip,
                None,
                None,
                |_| {},
            );
        }

        let clip_layers = scene
            .commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::PushClipLayer(_)))
            .count();
        let effect_layers = scene
            .commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::PushLayer(_)))
            .count();
        let popped_layers = scene
            .commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::PopLayer))
            .count();

        assert_eq!(clip_layers, LAYER_COUNT / 2);
        assert_eq!(effect_layers, LAYER_COUNT / 2);
        assert_eq!(popped_layers, LAYER_COUNT);
    }
}
