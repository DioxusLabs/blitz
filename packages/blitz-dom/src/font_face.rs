use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc::Sender};

use blitz_traits::net::{AbortSignal, Bytes, NetProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::ShellProvider;
use linebender_resource_handle::Blob;
use parley::FontContext;
use parley::fontique::{FontId, PendingFontStatus};
use style::font_face::FontFaceSourceFormatKeyword;
use url::Url;

use crate::document::DocumentEvent;
use crate::net::{
    FontFaceDeclaration, FontFaceHandler, FontFaceOverrides, ResourceHandler, stamped_request,
};

/// A change to apply to every font context between `resolve()` calls.
pub(crate) enum FontUpdate {
    /// The source of a pending face finished loading (`None` if it failed).
    Resolve { key: FontId, font: Option<Blob<u8>> },
    /// Font bytes that were never registered as a pending face.
    Register {
        font: Blob<u8>,
        overrides: FontFaceOverrides,
    },
}

impl FontUpdate {
    pub fn apply(&self, ctx: &mut FontContext) {
        match self {
            Self::Resolve { key, font } => {
                let resolved = font
                    .as_ref()
                    .is_some_and(|font| ctx.collection.resolve_pending_font(*key, font.clone(), 0));
                if !resolved {
                    ctx.collection
                        .set_pending_font_status(*key, PendingFontStatus::Failed);
                }
            }
            Self::Register { font, overrides } => {
                ctx.collection
                    .register_all_fonts(font.clone(), Some(overrides.as_font_info()));
            }
        }
    }
}

#[derive(Default)]
enum SourceState {
    /// No face using this source has been needed yet.
    #[default]
    Idle,
    Loading,
    Loaded(Blob<u8>),
    Failed,
}

/// A font URL, shared by every face that names it.
struct FontSource {
    state: SourceState,
    format: FontFaceSourceFormatKeyword,
    node_id: Option<NodeId>,
    /// Pending faces to resolve when the source completes, in registration order.
    faces: Vec<FontId>,
}

#[derive(Default)]
struct State {
    /// The source of each pending face.
    faces: HashMap<FontId, Url>,
    sources: HashMap<Url, FontSource>,
    updates: Vec<FontUpdate>,
    /// Sources that became `Loading` and whose fetch has not been started yet.
    requests: Vec<Url>,
}

pub(crate) struct FontFaceLoader {
    state: Mutex<State>,
    net: Arc<dyn NetProvider>,
    shell: Arc<dyn ShellProvider>,
    tx: Sender<DocumentEvent>,
    doc_id: usize,
    abort_signal: Option<AbortSignal>,
}

impl FontFaceLoader {
    pub fn new(
        net: Arc<dyn NetProvider>,
        shell: Arc<dyn ShellProvider>,
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        abort_signal: Option<AbortSignal>,
    ) -> Self {
        Self {
            state: Mutex::default(),
            net,
            shell,
            tx,
            doc_id,
            abort_signal,
        }
    }

    pub fn register(&self, declaration: FontFaceDeclaration, node_id: Option<NodeId>) -> FontId {
        let mut state = self.state.lock().unwrap();
        let state = &mut *state;
        let key = FontId::new();
        let source = state
            .sources
            .entry(declaration.url.clone())
            .or_insert_with(|| FontSource {
                state: SourceState::Idle,
                format: declaration.format,
                node_id,
                faces: Vec::new(),
            });
        let font = match &source.state {
            SourceState::Loaded(font) => Some(Some(font.clone())),
            SourceState::Failed => Some(None),
            SourceState::Idle | SourceState::Loading => None,
        };
        if let Some(font) = font {
            state.updates.push(FontUpdate::Resolve { key, font });
        } else {
            source.faces.push(key);
            state.faces.insert(key, declaration.url);
        }
        key
    }

    /// Queues a fetch for the source of each pending face that layout or metrics
    /// requested from `font_ctx`, the calling thread's font context. Callable from any
    /// thread; `start_requests` starts the fetches.
    ///
    /// The faces are marked as loading in `font_ctx`, which stops its queries from
    /// requesting them again. So only a thread's first demand for a face takes the lock
    /// shared by all threads.
    pub fn request(&self, font_ctx: &mut FontContext) {
        let keys: Vec<FontId> = font_ctx.collection.take_font_requests().collect();
        self.request_faces(font_ctx, keys.into_iter());
    }

    fn request_faces(&self, font_ctx: &mut FontContext, keys: impl Iterator<Item = FontId>) {
        let keys: Vec<FontId> = keys
            .filter(|key| {
                font_ctx
                    .collection
                    .set_pending_font_status(*key, PendingFontStatus::Loading)
            })
            .collect();
        if keys.is_empty() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        let state = &mut *state;
        for key in keys {
            let Some(url) = state.faces.get(&key) else {
                continue;
            };
            let source = state.sources.get_mut(url).unwrap();
            if matches!(source.state, SourceState::Idle) {
                source.state = SourceState::Loading;
                state.requests.push(url.clone());
            }
        }
    }

    pub fn start_requests(&self) {
        let requests: Vec<_> = {
            let mut state = self.state.lock().unwrap();
            let urls = std::mem::take(&mut state.requests);
            urls.into_iter()
                .map(|url| {
                    let source = &state.sources[&url];
                    (url, source.format, source.node_id)
                })
                .collect()
        };
        for (url, format, node_id) in requests {
            if self.net.is_noop() {
                self.complete(url, None);
                continue;
            }
            self.net.fetch(
                self.doc_id,
                stamped_request(url.clone(), self.abort_signal.as_ref()),
                ResourceHandler::boxed(
                    self.tx.clone(),
                    self.doc_id,
                    node_id,
                    self.shell.clone(),
                    FontFaceHandler::new(
                        format,
                        url,
                        self.tx.clone(),
                        self.doc_id,
                        node_id,
                        self.shell.clone(),
                    ),
                ),
            );
        }
    }

    pub fn complete(&self, url: Url, bytes: Option<Bytes>) {
        let font = bytes.map(|bytes| Blob::new(Arc::new(bytes)));
        let mut state = self.state.lock().unwrap();
        let state = &mut *state;
        let Some(source) = state.sources.get_mut(&url) else {
            return;
        };
        source.state = match &font {
            Some(font) => SourceState::Loaded(font.clone()),
            None => SourceState::Failed,
        };
        for key in std::mem::take(&mut source.faces) {
            state.faces.remove(&key);
            state.updates.push(FontUpdate::Resolve {
                key,
                font: font.clone(),
            });
        }
    }

    pub fn queue_font(&self, bytes: Bytes, overrides: FontFaceOverrides) {
        self.state
            .lock()
            .unwrap()
            .updates
            .push(FontUpdate::Register {
                font: Blob::new(Arc::new(bytes)),
                overrides,
            });
    }

    pub fn take_updates(&self) -> Vec<FontUpdate> {
        std::mem::take(&mut self.state.lock().unwrap().updates)
    }

    pub fn is_pending(&self) -> bool {
        let state = self.state.lock().unwrap();
        !state.updates.is_empty()
            || state
                .sources
                .values()
                .any(|source| matches!(source.state, SourceState::Loading))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseDocument, DocumentConfig};
    use blitz_traits::net::{NetHandler, Request};

    /// `Some(true)` once the family's face has bytes, `Some(false)` while it is pending
    /// or failed.
    fn loaded(doc: &BaseDocument, family: &str) -> Option<bool> {
        let mut fonts = doc.font_ctx.lock().unwrap();
        let family = fonts.collection.family_by_name(family)?;
        Some(family.default_font()?.load(None).is_some())
    }

    fn status(doc: &BaseDocument, key: FontId) -> Option<PendingFontStatus> {
        let mut fonts = doc.font_ctx.lock().unwrap();
        fonts.collection.pending_font_status(key)
    }

    /// The id of the family's face.
    fn id(doc: &BaseDocument, family: &str) -> FontId {
        let mut fonts = doc.font_ctx.lock().unwrap();
        let family = fonts.collection.family_by_name(family).unwrap();
        family.fonts()[0].id()
    }

    fn demand<const N: usize>(doc: &BaseDocument, keys: [FontId; N]) {
        let mut fonts = doc.font_ctx.lock().unwrap();
        doc.font_face_loader
            .request_faces(&mut fonts, keys.into_iter());
    }

    #[derive(Default)]
    struct Network(Mutex<Vec<(Request, Box<dyn NetHandler>)>>);

    impl NetProvider for Network {
        fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
            self.0.lock().unwrap().push((request, handler));
        }
    }

    fn face(family: &str, url: &str) -> FontFaceDeclaration {
        FontFaceDeclaration {
            url: Url::parse(url).unwrap(),
            format: FontFaceSourceFormatKeyword::Opentype,
            overrides: FontFaceOverrides {
                family_name: Some(family.into()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn deferred_fonts_deduplicate_workers_and_urls_and_batch_arrivals() {
        let net = Arc::new(Network::default());
        let mut doc = BaseDocument::new(DocumentConfig {
            net_provider: Some(net.clone()),
            ..Default::default()
        });
        doc.register_font_faces(
            vec![
                face("A", "https://example.com/shared.otf"),
                face("B", "https://example.com/shared.otf"),
                face("Unused", "https://example.com/unused.otf"),
            ],
            None,
        );
        assert!(net.0.lock().unwrap().is_empty());
        assert!(!doc.has_pending_font_loads());
        demand(&doc, []);
        let (a, b, unused) = (id(&doc, "A"), id(&doc, "B"), id(&doc, "Unused"));
        // Each layout thread has its own copy of the font context.
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let loader = &doc.font_face_loader;
                let mut fonts = doc.font_ctx.lock().unwrap().clone();
                scope.spawn(move || {
                    loader.request_faces(&mut fonts, [a, a, b].into_iter());
                    let status = fonts.collection.pending_font_status(a);
                    assert_eq!(status, Some(PendingFontStatus::Loading));
                });
            }
        });
        assert_eq!(status(&doc, a), Some(PendingFontStatus::Unloaded));
        demand(&doc, [a]);
        assert_eq!(status(&doc, a), Some(PendingFontStatus::Loading));
        assert_eq!(status(&doc, unused), Some(PendingFontStatus::Unloaded));
        doc.font_face_loader.start_requests();
        assert_eq!(net.0.lock().unwrap().len(), 1);
        assert!(doc.has_pending_font_loads());
        let (request, handler) = net.0.lock().unwrap().pop().unwrap();
        handler.bytes(
            request.url.to_string(),
            Bytes::from_static(crate::BULLET_FONT),
        );
        doc.handle_messages();
        assert_eq!(loaded(&doc, "A"), Some(false));
        doc.flush_pending_fonts();
        assert!(!doc.has_pending_font_loads());
        assert_eq!(loaded(&doc, "A"), Some(true));
        assert_eq!(loaded(&doc, "B"), Some(true));
        assert_eq!(loaded(&doc, "Unused"), Some(false));
    }

    #[test]
    fn failed_fonts_are_marked_failed_without_retrying() {
        let net = Arc::new(Network::default());
        let mut doc = BaseDocument::new(DocumentConfig {
            net_provider: Some(net.clone()),
            ..Default::default()
        });
        doc.register_font_faces(vec![face("A", "https://example.com/missing.otf")], None);
        let a = id(&doc, "A");
        demand(&doc, [a]);
        doc.font_face_loader.start_requests();
        let (request, handler) = net.0.lock().unwrap().pop().unwrap();
        handler.error(request.url.to_string(), "missing".into());
        doc.handle_messages();
        doc.flush_pending_fonts();
        assert!(!doc.has_pending_font_loads());
        assert_eq!(loaded(&doc, "A"), Some(false));
        assert_eq!(status(&doc, a), Some(PendingFontStatus::Failed));
        demand(&doc, [a]);
        doc.font_face_loader.start_requests();
        assert!(net.0.lock().unwrap().is_empty());
    }

    #[test]
    fn dropped_font_handler_completes_as_failure() {
        let net = Arc::new(Network::default());
        let mut doc = BaseDocument::new(DocumentConfig {
            net_provider: Some(net.clone()),
            ..Default::default()
        });
        doc.register_font_faces(vec![face("A", "https://example.com/dropped.otf")], None);
        let a = id(&doc, "A");
        demand(&doc, [a]);
        doc.font_face_loader.start_requests();
        assert!(doc.has_pending_font_loads());
        let (_, handler) = net.0.lock().unwrap().pop().unwrap();
        drop(handler);
        doc.handle_messages();
        doc.flush_pending_fonts();
        assert!(!doc.has_pending_font_loads());
        assert_eq!(loaded(&doc, "A"), Some(false));
        assert_eq!(status(&doc, a), Some(PendingFontStatus::Failed));
    }
}
