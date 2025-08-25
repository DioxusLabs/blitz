use crate::{BaseDocument, net::ImageHandler, node::BackgroundImageData, util::ImageType};
use blitz_traits::net::Request;
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::servo::url::ComputedUrl;
use style::values::generics::image::Image as StyloImage;

impl BaseDocument {
    /// Walk the whole tree, converting styles to layout
    pub fn flush_styles_to_layout(&mut self, node_id: usize) {
        let doc_id = self.id();

        let display = {
            let node = self.nodes.get_mut(node_id).unwrap();
            let stylo_element_data = node.stylo_element_data.borrow();
            let primary_styles = stylo_element_data
                .as_ref()
                .and_then(|data| data.styles.get_primary());

            let Some(style) = primary_styles else {
                return;
            };

            node.style = stylo_taffy::to_taffy_style(style);
            node.display_constructed_as = style.clone_display();

            // Flush background image from style to dedicated storage on the node
            // TODO: handle multiple background images
            if let Some(elem) = node.data.downcast_element_mut() {
                let style_bgs = &style.get_background().background_image.0;
                let elem_bgs = &mut elem.background_images;

                let len = style_bgs.len();
                elem_bgs.resize_with(len, || None);

                for idx in 0..len {
                    let background_image = &style_bgs[idx];
                    let new_bg_image = match background_image {
                        StyloImage::Url(ComputedUrl::Valid(new_url)) => {
                            let old_bg_image = elem_bgs[idx].as_ref();
                            let old_bg_image_url = old_bg_image.map(|data| &data.url);
                            if old_bg_image_url.is_some_and(|old_url| **new_url == **old_url) {
                                break;
                            }

                            self.net_provider.fetch(
                                doc_id,
                                Request::get((**new_url).clone()),
                                Box::new(ImageHandler::new(node_id, ImageType::Background(idx))),
                            );

                            let bg_image_data = BackgroundImageData::new(new_url.clone());
                            Some(bg_image_data)
                        }
                        _ => None,
                    };

                    // Element will always exist due to resize_with above
                    elem_bgs[idx] = new_bg_image;
                }
            }

            // Clear Taffy cache
            // TODO: smarter cache invalidation
            node.cache.clear();

            node.style.display
        };

        // If the node has children, then take those children and...
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        if let Some(mut children) = children {
            // Recursively call flush_styles_to_layout on each child
            for child in children.iter() {
                self.flush_styles_to_layout(*child);
            }

            // If the node is a Flexbox or Grid node then sort by css order property
            if matches!(display, taffy::Display::Flex | taffy::Display::Grid) {
                children.sort_by(|left, right| {
                    let left_node = self.nodes.get(*left).unwrap();
                    let right_node = self.nodes.get(*right).unwrap();
                    left_node.order().cmp(&right_node.order())
                });
            }

            // Put children back
            *self.nodes[node_id].layout_children.borrow_mut() = Some(children);

            // Sort paint_children in place
            self.nodes[node_id]
                .paint_children
                .borrow_mut()
                .as_mut()
                .unwrap()
                .sort_by(|left, right| {
                    let left_node = self.nodes.get(*left).unwrap();
                    let right_node = self.nodes.get(*right).unwrap();
                    left_node
                        .z_index()
                        .cmp(&right_node.z_index())
                        .then_with(|| {
                            fn position_to_order(pos: Position) -> u8 {
                                match pos {
                                    Position::Static | Position::Relative | Position::Sticky => 0,
                                    Position::Absolute | Position::Fixed => 1,
                                }
                            }
                            let left_position = left_node
                                .primary_styles()
                                .map(|s| position_to_order(s.clone_position()))
                                .unwrap_or(0);
                            let right_position = right_node
                                .primary_styles()
                                .map(|s| position_to_order(s.clone_position()))
                                .unwrap_or(0);

                            left_position.cmp(&right_position)
                        })
                })
        }
    }
}
