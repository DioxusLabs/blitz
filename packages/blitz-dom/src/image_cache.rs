//! Cache of loaded images, keyed by URL.

use std::collections::{HashMap, HashSet};

use crate::node::ImageData;

/// How many bytes of decoded images that no node uses the cache keeps for reuse.
const UNUSED_BYTES: usize = 32 << 20;

/// Loaded images, keyed by URL, so that elements showing the same image share it
/// without fetching and decoding it again.
///
/// Images that no node uses any more are kept only up to [`UNUSED_BYTES`], least
/// recently used dropped first (as browsers bound their memory caches), so a page that
/// keeps showing new images does not keep every one it has ever shown.
#[derive(Default)]
pub(crate) struct ImageCache {
    entries: HashMap<String, Entry>,
    clock: u64,
}

struct Entry {
    image: ImageData,
    used: u64,
}

impl ImageCache {
    pub(crate) fn get(&mut self, url: &str) -> Option<&ImageData> {
        self.clock += 1;
        let entry = self.entries.get_mut(url)?;
        entry.used = self.clock;
        Some(&entry.image)
    }

    pub(crate) fn insert(&mut self, url: String, image: ImageData) {
        self.clock += 1;
        let used = self.clock;
        self.entries.insert(url, Entry { image, used });
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Drops images not in `in_use` (see [`identity`]), least recently used first,
    /// until those left take at most [`UNUSED_BYTES`].
    pub(crate) fn evict_unused(&mut self, in_use: &HashSet<u64>) {
        let mut unused: Vec<(u64, usize, &String)> = self
            .entries
            .iter()
            .filter(|(_, entry)| identity(&entry.image).is_some_and(|id| !in_use.contains(&id)))
            .map(|(url, entry)| (entry.used, bytes(&entry.image), url))
            .collect();
        let mut total: usize = unused.iter().map(|(_, bytes, _)| bytes).sum();
        if total <= UNUSED_BYTES {
            return;
        }
        unused.sort_unstable_by_key(|(used, _, _)| *used);
        let mut dropped = Vec::new();
        for (_, bytes, url) in unused {
            if total <= UNUSED_BYTES {
                break;
            }
            total -= bytes;
            dropped.push(url.clone());
        }
        for url in dropped {
            self.entries.remove(&url);
        }
    }
}

/// What nodes holding the same decoded image share.
pub(crate) fn identity(image: &ImageData) -> Option<u64> {
    match image {
        ImageData::Raster(raster) => Some(raster.data.id()),
        #[cfg(feature = "svg")]
        ImageData::Svg(svg) => Some(std::sync::Arc::as_ptr(&svg.tree) as usize as u64),
        ImageData::None => None,
    }
}

fn bytes(image: &ImageData) -> usize {
    match image {
        ImageData::Raster(raster) => raster.data.len(),
        _ => 0,
    }
}
