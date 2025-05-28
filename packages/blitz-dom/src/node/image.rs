use super::{ImageContext, SpecialElementData};
use crate::{BaseDocument, net::ImageHandler, util::ImageType};
use blitz_traits::net::{AbortController, Request};
use markup5ever::local_name;
use mime::Mime;
use std::{
    collections::HashSet,
    ops::{Deref, DerefMut},
};
use style::{
    parser::ParserContext,
    stylesheets::{CssRuleType, Origin, UrlExtraData},
    values::specified::source_size_list::SourceSizeList,
};
use style_traits::ParsingMode;
use url::Url;

impl BaseDocument {
    #[inline]
    pub fn is_img_node(&self, node_id: usize) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        node.data.is_element_with_tag_name(&local_name!("img"))
    }

    #[inline]
    pub fn is_picture_node(&self, node_id: usize) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        node.data.is_element_with_tag_name(&local_name!("picture"))
    }

    pub(crate) fn load_image(&mut self, target_id: usize) {
        let Some(selected_source) = self.select_image_source(target_id) else {
            return;
        };

        let src = self.resolve_url(&selected_source.url);

        let node = self.get_node_mut(target_id).unwrap();
        let Some(data) = node.element_data_mut() else {
            return;
        };

        let controller = AbortController::default();
        let signal = controller.signal.clone();

        match &mut data.special_data {
            SpecialElementData::Image(context)
                if !context
                    .selected_source
                    .is_same_image_source(&selected_source) =>
            {
                context.selected_source = selected_source;
                if let Some(controller) = context.controller.replace(controller) {
                    controller.abort();
                }
            }
            SpecialElementData::None => {
                data.special_data = SpecialElementData::Image(Box::new(
                    ImageContext::new_with_controller(selected_source, controller),
                ));
            }
            _ => return,
        }

        self.net_provider.fetch(
            self.id(),
            Request::get(src).signal(signal),
            Box::new(ImageHandler::new(target_id, ImageType::Image)),
        );
    }

    // https://html.spec.whatwg.org/multipage/images.html#reacting-to-environment-changes
    pub(crate) fn environment_changes_with_image(&mut self, node_id: usize) {
        // 2. If the img element does not use srcset or picture.
        if !self.use_srcset_or_picture(node_id) {
            return;
        }

        self.load_image(node_id);
    }

    fn use_srcset_or_picture(&self, node_id: usize) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };

        if node.attr(local_name!("srcset")).is_some() {
            return true;
        }

        if let Some(parent_id) = node.parent {
            if self.is_picture_node(parent_id) {
                return true;
            }
        }

        false
    }

    /// Selecting an image source
    ///
    /// https://html.spec.whatwg.org/multipage/#select-an-image-source
    fn select_image_source(&self, el_id: usize) -> Option<ImageSource> {
        let source_set = self.get_source_set(el_id)?;
        let image_sources = source_set.image_sources;
        let len = image_sources.len();

        if len == 0 {
            return None;
        }

        // 1. If an entry b in sourceSet has the same associated pixel density descriptor as
        // an earlier entry a in sourceSet, then remove entry b. Repeat this step until none
        // of the entries in sourceSet have the same associated pixel density descriptor as
        // an earlier entry.
        let mut seen = HashSet::new();
        let image_sources = image_sources
            .into_inner()
            .into_iter()
            .filter(|image_source| {
                let density = image_source.descriptor.density.unwrap();
                seen.insert(density.to_bits())
            })
            .collect::<Vec<_>>();

        let device_pixel_ratio = self.viewport.scale_f64();

        // 2.2 In an implementation-defined manner, choose one image source from sourceSet. Let this be selectedSource.
        let image_source = image_sources
            .iter()
            .find(|image_source| {
                let density = image_source.descriptor.density.unwrap();
                density >= device_pixel_ratio
            })
            .unwrap_or_else(|| image_sources.last().unwrap());

        Some(image_source.clone())
    }

    /// https://html.spec.whatwg.org/multipage/#update-the-source-set
    fn get_source_set(&self, el_id: usize) -> Option<SourceSet> {
        // 2. Let elements be « el ».
        let el = self.get_node(el_id)?;

        // 3. If el is an img element whose parent node is a picture element,
        // then replace the contents of elements with el's parent node's child elements,
        // retaining relative order.
        let elements = if let Some(parent_id) = el.parent {
            if self.is_picture_node(parent_id) {
                let parent = &self.nodes[parent_id];
                parent.children.clone()
            } else {
                vec![el_id]
            }
        } else {
            vec![el_id]
        };

        // 5. For each child in elements.
        for element in elements {
            // 5.1 If child is el.
            if element == el_id {
                let element = self.get_node(element)?;
                // 5.1.1 Let default source be the empty string.
                // 5.1.2 Let srcset be the empty string.
                let mut source_set = SourceSet::new();

                // 5.1.4 If el is an img element that has a srcset attribute,
                // then set srcset to that attribute's value.
                if let Some(srcset) = element.attr(local_name!("srcset")) {
                    source_set.image_sources = ImageSourceList::parse(srcset);
                }

                // 5.1.6 If el is an img element that has a sizes attribute,
                // then set sizes to that attribute's value.
                if let Some(sizes) = element.attr(local_name!("sizes")) {
                    source_set.source_size = self.parse_sizes_attribute(sizes);
                }

                // 5.1.8. If el is an img element that has a src attribute,
                // then set default source to that attribute's value.
                let src = element.attr(local_name!("src"));
                if let Some(src) = src {
                    if !src.is_empty() {
                        source_set
                            .image_sources
                            .push(ImageSource::new(src.to_string()))
                    }
                }

                self.normalise_source_densities(&mut source_set);

                // 5.1.11. Return.
                return Some(source_set);
            }

            let Some(element) = self.get_node(element) else {
                continue;
            };
            // 5.2 If child is not a source element, then continue.
            if element
                .element_data()
                .is_none_or(|data| data.name.local != local_name!("source"))
            {
                continue;
            }

            let mut source_set = SourceSet::new();

            // 5.3 If child does not have a srcset attribute, continue to the next child.
            let Some(srcset) = element.attr(local_name!("srcset")) else {
                continue;
            };
            // 5.4 Parse child's srcset attribute and let the returned source set be source set.
            source_set.image_sources = ImageSourceList::parse(srcset);
            // 5.5 If source set has zero image sources, continue to the next child.
            if source_set.image_sources.is_empty() {
                continue;
            }

            // 5.6 If child has a media attribute, and its value does not match the environment,
            // continue to the next child.
            if let Some(media) = element.attr(local_name!("media")) {
                if !self.match_media(media) {
                    continue;
                }
            }

            // 5.7 Parse child's sizes attribute with img, and let source set's
            // source size be the returned value.
            if let Some(sizes) = element.attr(local_name!("sizes")) {
                source_set.source_size = self.parse_sizes_attribute(sizes);
            }

            // 5.8 If child has a type attribute, and its value is an unknown or unsupported MIME type,
            // continue to the next child.
            if let Some(type_) = element.attr(local_name!("type")) {
                let Ok(mime) = type_.parse::<Mime>() else {
                    continue;
                };
                if mime.type_() != mime::IMAGE {
                    continue;
                }

                // Unsupported mime types
                if mime.essence_str() != "image/svg+xml"
                    && image::ImageFormat::from_mime_type(type_).is_none()
                {
                    continue;
                }
            }

            // 5.10 Normalize the source densities of source set.
            self.normalise_source_densities(&mut source_set);

            // 5.12 Return.
            return Some(source_set);
        }

        None
    }

    /// Parsing a `sizes` attribute.
    ///
    /// https://html.spec.whatwg.org/multipage/images.html#parsing-a-sizes-attribute
    fn parse_sizes_attribute(&self, input: &str) -> SourceSizeList {
        let mut input = cssparser::ParserInput::new(input);
        let mut parser = cssparser::Parser::new(&mut input);

        let url_data = UrlExtraData::from(
            self.base_url
                .clone()
                .unwrap_or_else(|| "about:blank".parse::<Url>().unwrap()),
        );
        let quirks_mode = self.stylist.quirks_mode();
        let context = ParserContext::new(
            Origin::Author,
            &url_data,
            Some(CssRuleType::Style),
            ParsingMode::empty(),
            quirks_mode,
            Default::default(),
            None,
            None,
        );

        SourceSizeList::parse(&context, &mut parser)
    }

    /// https://html.spec.whatwg.org/multipage/images.html#normalizing-the-source-densities
    fn normalise_source_densities(&self, source_set: &mut SourceSet) {
        // 1. Let source size be source set's source size.
        let source_size = &mut source_set.source_size;

        let source_size_length =
            source_size.evaluate(self.stylist.device(), self.stylist.quirks_mode());

        // 2. For each image source in source set.
        for imgsource in source_set.image_sources.iter_mut() {
            // 2.1 If the image source has a pixel density descriptor, continue to the next image source.
            if imgsource.descriptor.density.is_some() {
                continue;
            }
            // 2.2 Otherwise, if the image source has a width descriptor, replace the width descriptor with
            // a pixel density descriptor with a value of the width descriptor value divided by source size and a unit of x.
            if let Some(width) = imgsource.descriptor.width {
                imgsource.descriptor.density = Some(width as f64 / source_size_length.to_f64_px());
            } else {
                // 2.3 Otherwise, give the image source a pixel density descriptor of 1x.
                imgsource.descriptor.density = Some(1f64);
            }
        }
    }
}

#[derive(Debug)]
struct SourceSet {
    // srcset
    image_sources: ImageSourceList,
    source_size: SourceSizeList,
}

impl SourceSet {
    fn new() -> Self {
        Self {
            image_sources: Default::default(),
            source_size: SourceSizeList::empty(),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct ImageSourceList(Vec<ImageSource>);

impl Deref for ImageSourceList {
    type Target = Vec<ImageSource>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ImageSourceList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ImageSourceList {
    /// Parse an `srcset` attribute.
    fn parse(input: &str) -> Self {
        let mut candidates = vec![];

        for image_source_str in input.split(",") {
            let image_source_str = image_source_str.trim();
            if image_source_str.is_empty() {
                continue;
            }

            if let Some(image_source) = ImageSource::parse(image_source_str) {
                candidates.push(image_source);
            }
        }

        Self(candidates)
    }

    fn into_inner(self) -> Vec<ImageSource> {
        self.0
    }
}

/// Srcset attributes
///
/// https://html.spec.whatwg.org/multipage/images.html#srcset-attributes
#[derive(Debug, PartialEq, Clone)]
pub struct ImageSource {
    pub url: String,
    pub descriptor: Descriptor,
}

impl ImageSource {
    pub fn new(url: String) -> Self {
        Self {
            url,
            descriptor: Default::default(),
        }
    }

    #[inline]
    fn is_same_image_source(&self, other: &Self) -> bool {
        self.url == other.url && self.descriptor.density == other.descriptor.density
    }

    fn parse(input: &str) -> Option<Self> {
        let image_source_split = input.split_ascii_whitespace().collect::<Vec<&str>>();
        let len = image_source_split.len();

        match len {
            1 => Some(Self {
                url: image_source_split[0].to_string(),
                descriptor: Default::default(),
            }),
            2 => {
                let descriptor = Descriptor::parse(image_source_split[1])?;

                Some(Self {
                    url: image_source_split[0].to_string(),
                    descriptor,
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct Descriptor {
    pub width: Option<u32>,
    pub density: Option<f64>,
}

impl Descriptor {
    fn parse(input: &str) -> Option<Self> {
        if input.len() < 2 {
            return None;
        }
        let (number, unit) = input.split_at(input.len() - 1);
        match unit {
            "w" => match number.parse::<u32>() {
                Ok(number) if number > 0 => Some(Self {
                    width: Some(number),
                    density: Default::default(),
                }),
                _ => None,
            },
            "x" => match number.parse::<f64>() {
                Ok(number) if number.is_normal() && number > 0. => Some(Self {
                    width: Default::default(),
                    density: Some(number),
                }),
                _ => None,
            },
            _ => None,
        }
    }
}

#[test]
fn test_parse_image_source_list() {
    let list = ImageSourceList::parse("/url.jpg, /url.jpg 2x, /url.jpg 2w");
    assert_eq!(
        list,
        ImageSourceList(vec![
            ImageSource::new("/url.jpg".to_string()),
            ImageSource {
                url: "/url.jpg".to_string(),
                descriptor: Descriptor {
                    width: None,
                    density: Some(2.),
                },
            },
            ImageSource {
                url: "/url.jpg".to_string(),
                descriptor: Descriptor {
                    width: Some(2),
                    density: None,
                },
            },
        ])
    );
}
