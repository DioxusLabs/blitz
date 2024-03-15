use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use dioxus_native_core::{node::OwnedAttributeValue, prelude::*};
use dioxus_native_core_macro::partial_derive_state;
use shipyard::Component;
use vello::peniko::{Blob, Format, Image};

#[derive(Default, Clone)]
pub struct ImageContext {
    cache: Arc<RwLock<ImageCache>>,
}

impl ImageContext {
    pub fn load_file(&self, path: impl AsRef<Path>) -> Result<Arc<Image>, ImageError> {
        self.cache.write().unwrap().load_file(path)
    }
}

#[derive(Default)]
pub struct ImageCache {
    files: HashMap<PathBuf, Arc<Image>>,
}

impl ImageCache {
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<Arc<Image>, ImageError> {
        let path = path.as_ref();
        let contains_image = self.files.contains_key(path);
        if !contains_image {
            let data = std::fs::read(path)?;
            let image = decode_image(&data)?;
            self.files.insert(path.to_path_buf(), Arc::new(image));
        }
        Ok(self.files.get(path).unwrap().clone())
    }
}

fn decode_image(data: &[u8]) -> Result<Image, ImageError> {
    let image = image::io::Reader::new(std::io::Cursor::new(data))
        .with_guessed_format()?
        .decode()?;
    let width = image.width();
    let height = image.height();
    let data = Arc::new(image.into_rgba8().into_vec());
    let blob = Blob::new(data);
    Ok(Image::new(blob, Format::Rgba8, width, height))
}

#[derive(Debug)]
pub enum ImageError {
    Io(io::Error),
    Image(image::ImageError),
}

impl From<io::Error> for ImageError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<image::ImageError> for ImageError {
    fn from(err: image::ImageError) -> Self {
        Self::Image(err)
    }
}

#[derive(Debug, Default, PartialEq, Clone, Component)]
pub(crate) struct LoadedImage(pub Option<Arc<Image>>);

#[partial_derive_state]
impl State for LoadedImage {
    type ChildDependencies = ();
    type ParentDependencies = ();
    type NodeDependencies = ();
    const NODE_MASK: NodeMaskBuilder<'static> = NodeMaskBuilder::new()
        .with_tag()
        .with_attrs(AttributeMaskBuilder::Some(&["src"]));

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        _: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        ctx: &SendAnyMap,
    ) -> bool {
        let mut new = None;
        if let Some(OwnedAttributeValue::Text(image)) =
            node_view.attributes().and_then(|mut attrs| {
                attrs
                    .find(|attr| attr.attribute.name == "src")
                    .map(|attr| attr.value)
            })
        {
            let image_ctx: &ImageContext = ctx.get().expect("ImageContext not found");
            new = Some(image_ctx.load_file(image).unwrap());
        }
        if self.0 != new {
            self.0 = new;
            true
        } else {
            false
        }
    }
}
