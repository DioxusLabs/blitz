//! Image element (UIImageView) implementation
//!
//! Maps `<img>` elements to UIImageView.

use std::cell::Cell;

use blitz_dom::Node;
use blitz_dom::node::{ImageData, RasterImageData};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::{UIImage, UIImageView, UIView, UIViewContentMode};

// =============================================================================
// BlitzImageView - Custom UIImageView
// =============================================================================

/// Ivars for BlitzImageView
#[derive(Default)]
pub struct BlitzImageViewIvars {
    pub node_id: Cell<usize>,
}

define_class!(
    /// A UIImageView subclass that tracks its blitz-dom node ID.
    #[unsafe(super(UIImageView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzImageView"]
    #[ivars = BlitzImageViewIvars]
    pub struct BlitzImageView;

    unsafe impl NSObjectProtocol for BlitzImageView {}
);

impl BlitzImageView {
    /// Create a new BlitzImageView.
    pub fn new(mtm: MainThreadMarker, node_id: usize) -> Retained<Self> {
        let ivars = BlitzImageViewIvars {
            node_id: Cell::new(node_id),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let image_view: Retained<Self> = unsafe { msg_send![super(this), init] };

        // Default content mode to aspect fit
        unsafe {
            image_view.setContentMode(UIViewContentMode::ScaleAspectFit);
            // Clip to bounds
            image_view.setClipsToBounds(true);
        }

        image_view
    }

    /// Get the node ID.
    pub fn node_id(&self) -> usize {
        self.ivars().node_id.get()
    }
}

/// Create a UIImageView for an img element.
pub fn create_image_view(mtm: MainThreadMarker, node: &Node, node_id: usize) -> Retained<UIView> {
    let image_view = BlitzImageView::new(mtm, node_id);

    // Try to set initial image
    set_image_from_node(&image_view, node);

    // Cast to UIView
    unsafe { Retained::cast(image_view) }
}

/// Update a UIImageView with new node data.
pub fn update_image_view(view: &UIView, node: &Node) {
    // SAFETY: We only call this for ImageView element types
    let image_view: &UIImageView = unsafe { std::mem::transmute(view) };
    set_image_from_node(image_view, node);
}

/// Set image content from node's image data.
fn set_image_from_node(image_view: &UIImageView, node: &Node) {
    let Some(element_data) = node.element_data() else {
        return;
    };

    // Check for image data in special_data
    if let blitz_dom::node::SpecialElementData::Image(ref image_data) = element_data.special_data {
        match image_data.as_ref() {
            ImageData::Raster(raster) => {
                if let Some(ui_image) = create_ui_image_from_raster(raster) {
                    unsafe { image_view.setImage(Some(&ui_image)) };
                }
            }
            ImageData::Svg(_svg_tree) => {
                // TODO: Render SVG to UIImage
                // For now, SVG support is not implemented
                #[cfg(debug_assertions)]
                println!("[BlitzImageView] SVG images not yet supported");
            }
            ImageData::None => {
                unsafe { image_view.setImage(None) };
            }
        }
    }
}

/// Create a UIImage from raster image data.
fn create_ui_image_from_raster(_raster: &RasterImageData) -> Option<Retained<UIImage>> {
    // RasterImageData contains width, height, and RGBA8 data
    // We need to create a CGImage and then UIImage from it

    // TODO: Implement proper image conversion
    // This requires using Core Graphics to create a CGImage from raw pixels
    // For now, return None

    #[cfg(debug_assertions)]
    println!(
        "[BlitzImageView] Image conversion not yet implemented ({}x{})",
        _raster.width, _raster.height
    );

    None
}
