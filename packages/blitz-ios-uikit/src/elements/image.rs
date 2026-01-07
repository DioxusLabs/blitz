//! Image element (UIImageView) implementation
//!
//! Maps `<img>` elements to UIImageView.

use std::cell::Cell;

use blitz_dom::Node;
use blitz_dom::node::{ImageData, RasterImageData};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_core_foundation::CFData;
use objc2_core_graphics::{CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo};
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

        unsafe {
            // Default content mode to aspect fit
            image_view.setContentMode(UIViewContentMode::ScaleAspectFit);
            // Clip to bounds for border-radius support
            image_view.setClipsToBounds(true);
            // Enable user interaction for drag/copy/etc
            image_view.setUserInteractionEnabled(true);
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
        println!("[BlitzImageView] No element_data for image node");
        return;
    };

    // Check for image data in special_data
    if let blitz_dom::node::SpecialElementData::Image(ref image_data) = element_data.special_data {
        match image_data.as_ref() {
            ImageData::Raster(raster) => {
                println!(
                    "[BlitzImageView] Raster image: {}x{}, {} bytes",
                    raster.width, raster.height, raster.data.len()
                );
                if let Some(ui_image) = create_ui_image_from_raster(raster) {
                    println!("[BlitzImageView] UIImage created successfully");
                    unsafe { image_view.setImage(Some(&ui_image)) };
                } else {
                    println!("[BlitzImageView] Failed to create UIImage from raster");
                }
            }
            ImageData::Svg(_svg_tree) => {
                println!("[BlitzImageView] SVG images not yet supported");
            }
            ImageData::None => {
                println!("[BlitzImageView] ImageData::None - no image loaded");
                unsafe { image_view.setImage(None) };
            }
        }
    } else {
        println!("[BlitzImageView] special_data is not Image type: {:?}",
            std::mem::discriminant(&element_data.special_data));
    }
}

/// Create a UIImage from raster image data.
fn create_ui_image_from_raster(raster: &RasterImageData) -> Option<Retained<UIImage>> {
    let width = raster.width as usize;
    let height = raster.height as usize;
    let bytes_per_pixel = 4; // RGBA
    let bits_per_component = 8;
    let bytes_per_row = width * bytes_per_pixel;

    // Get the raw RGBA data (Blob<u8> implements AsRef<[u8]>)
    let rgba_data: &[u8] = raster.data.as_ref();

    // Create CFData from the raw bytes
    let cf_data = CFData::from_buffer(rgba_data);

    // Create CGDataProvider from CFData
    let data_provider = CGDataProvider::with_cf_data(Some(&cf_data))?;

    // Create device RGB color space
    let color_space = CGColorSpace::new_device_rgb()?;

    // Create CGImage from the data
    // CGBitmapInfo combines byte order with alpha info
    // For RGBA with premultiplied alpha: ByteOrderDefault | PremultipliedLast
    let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0);

    let cg_image = unsafe {
        CGImage::new(
            width,
            height,
            bits_per_component,
            bits_per_component * bytes_per_pixel,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
            Some(&data_provider),
            std::ptr::null(), // decode array (null for default)
            false,            // shouldInterpolate
            CGColorRenderingIntent::RenderingIntentDefault,
        )?
    };

    // Create UIImage from CGImage
    let ui_image = unsafe { UIImage::imageWithCGImage(&cg_image) };

    Some(ui_image)
}
