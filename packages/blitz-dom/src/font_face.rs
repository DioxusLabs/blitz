use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc::Sender};

use blitz_traits::net::{AbortSignal, Bytes, NetProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::ShellProvider;
use linebender_resource_handle::Blob;
use parley::FontContext;
use parley::fontique::PendingFontState;
use url::Url;

use crate::document::DocumentEvent;
use crate::net::{
    FontFaceDeclaration, FontFaceHandler, FontFaceOverrides, ResourceHandler, stamped_request,
};

pub(crate) struct FontUpdate {
    key: Option<u64>,
    font: Option<Blob<u8>>,
    overrides: Option<FontFaceOverrides>,
}

impl FontUpdate {
    pub fn apply(&self, ctx: &mut FontContext) {
        if let Some(key) = self.key {
            if let Some(font) = &self.font {
                if ctx.collection.resolve_pending_font(key, font.clone()) {
                    return;
                }
            }
            ctx.collection.unregister_pending_font(key);
        } else if let (Some(font), Some(overrides)) = (&self.font, &self.overrides) {
            ctx.collection
                .register_fonts(font.clone(), Some(overrides.as_font_info()));
        }
    }

    pub fn has_bytes(&self) -> bool {
        self.font.is_some()
    }
}

struct PendingFace {
    url: Url,
    format: style::font_face::FontFaceSourceFormatKeyword,
    node_id: Option<NodeId>,
}

#[derive(Default)]
struct FontSource {
    loading: bool,
    result: Option<Option<Blob<u8>>>,
}

#[derive(Default)]
struct State {
    next_key: u64,
    faces: HashMap<u64, PendingFace>,
    sources: HashMap<Url, FontSource>,
    updates: Vec<FontUpdate>,
    requests: Vec<(
        Url,
        style::font_face::FontFaceSourceFormatKeyword,
        Option<NodeId>,
    )>,
}

pub(crate) struct FontFaceLoader {
    state: Mutex<State>,
    global: Arc<Mutex<FontContext>>,
    net: Arc<dyn NetProvider>,
    shell: Arc<dyn ShellProvider>,
    tx: Sender<DocumentEvent>,
    doc_id: usize,
    abort_signal: Option<AbortSignal>,
}

impl FontFaceLoader {
    pub fn new(
        global: Arc<Mutex<FontContext>>,
        net: Arc<dyn NetProvider>,
        shell: Arc<dyn ShellProvider>,
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        abort_signal: Option<AbortSignal>,
    ) -> Self {
        Self {
            state: Mutex::default(),
            global,
            net,
            shell,
            tx,
            doc_id,
            abort_signal,
        }
    }

    pub fn register(&self, declaration: FontFaceDeclaration, node_id: Option<NodeId>) -> u64 {
        let mut state = self.state.lock().unwrap();
        let key = state.next_key;
        state.next_key += 1;
        let cached = state
            .sources
            .entry(declaration.url.clone())
            .or_default()
            .result
            .clone();
        if let Some(font) = cached {
            state.updates.push(FontUpdate {
                key: Some(key),
                font,
                overrides: None,
            });
        } else {
            state.faces.insert(
                key,
                PendingFace {
                    url: declaration.url,
                    format: declaration.format,
                    node_id,
                },
            );
        }
        key
    }

    pub fn request(
        &self,
        keys: impl Iterator<Item = u64>,
        local: &mut FontContext,
        is_global: bool,
    ) {
        for key in keys {
            if local.collection.pending_font_state(key) != Some(PendingFontState::Unloaded) {
                continue;
            }
            local
                .collection
                .set_pending_font_state(key, PendingFontState::Loading);
            if !is_global {
                let mut global = self.global.lock().unwrap();
                if global.collection.pending_font_state(key) != Some(PendingFontState::Unloaded) {
                    continue;
                }
                global
                    .collection
                    .set_pending_font_state(key, PendingFontState::Loading);
            }
            let request = {
                let mut state = self.state.lock().unwrap();
                let Some(face) = state.faces.get(&key) else {
                    continue;
                };
                let url = face.url.clone();
                let format = face.format;
                let node_id = face.node_id;
                let source = state.sources.get_mut(&url).unwrap();
                if source.loading {
                    continue;
                }
                source.loading = true;
                (url, format, node_id)
            };
            self.state.lock().unwrap().requests.push(request);
        }
    }

    pub fn start_requests(&self) {
        let requests = std::mem::take(&mut self.state.lock().unwrap().requests);
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
        state.sources.entry(url.clone()).or_default().result = Some(font.clone());
        let mut keys: Vec<_> = state
            .faces
            .iter()
            .filter_map(|(key, face)| (face.url == url).then_some(*key))
            .collect();
        keys.sort_unstable();
        for key in keys {
            state.faces.remove(&key);
            state.updates.push(FontUpdate {
                key: Some(key),
                font: font.clone(),
                overrides: None,
            });
        }
    }

    pub fn queue_font(&self, bytes: Bytes, overrides: FontFaceOverrides) {
        self.state.lock().unwrap().updates.push(FontUpdate {
            key: None,
            font: Some(Blob::new(Arc::new(bytes))),
            overrides: Some(overrides),
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
                .any(|source| source.loading && source.result.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseDocument, DocumentConfig};
    use blitz_traits::net::{NetHandler, Request};
    use style::font_face::FontFaceSourceFormatKeyword;

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
        doc.font_face_loader
            .request(std::iter::empty(), &mut doc.font_ctx.lock().unwrap(), true);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let mut local = doc.font_ctx.lock().unwrap().clone();
                let loader = &doc.font_face_loader;
                scope.spawn(move || loader.request([0, 0, 1].into_iter(), &mut local, false));
            }
        });
        doc.font_face_loader.start_requests();
        assert_eq!(net.0.lock().unwrap().len(), 1);
        assert!(doc.has_pending_font_loads());
        let (request, handler) = net.0.lock().unwrap().pop().unwrap();
        handler.bytes(
            request.url.to_string(),
            Bytes::from_static(crate::BULLET_FONT),
        );
        doc.handle_messages();
        assert!(
            doc.font_ctx
                .lock()
                .unwrap()
                .collection
                .pending_font_state(0)
                .is_some()
        );
        doc.flush_pending_fonts();
        assert!(!doc.has_pending_font_loads());
        let mut fonts = doc.font_ctx.lock().unwrap();
        for key in [0, 1] {
            assert_eq!(fonts.collection.pending_font_state(key), None);
        }
        for name in ["A", "B"] {
            assert!(
                fonts
                    .collection
                    .family_by_name(name)
                    .unwrap()
                    .default_font()
                    .unwrap()
                    .load(None)
                    .is_some()
            );
        }
        assert_eq!(
            fonts.collection.pending_font_state(2),
            Some(PendingFontState::Unloaded)
        );
    }

    #[test]
    fn failed_fonts_are_removed_without_retrying() {
        let net = Arc::new(Network::default());
        let mut doc = BaseDocument::new(DocumentConfig {
            net_provider: Some(net.clone()),
            ..Default::default()
        });
        doc.register_font_faces(vec![face("A", "https://example.com/missing.otf")], None);
        doc.font_face_loader
            .request([0].into_iter(), &mut doc.font_ctx.lock().unwrap(), true);
        doc.font_face_loader.start_requests();
        let (request, handler) = net.0.lock().unwrap().pop().unwrap();
        handler.error(request.url.to_string(), "missing".into());
        doc.handle_messages();
        doc.flush_pending_fonts();
        assert!(!doc.has_pending_font_loads());
        assert_eq!(
            doc.font_ctx
                .lock()
                .unwrap()
                .collection
                .pending_font_state(0),
            None
        );
        doc.font_face_loader
            .request([0].into_iter(), &mut doc.font_ctx.lock().unwrap(), true);
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
        doc.font_face_loader
            .request([0].into_iter(), &mut doc.font_ctx.lock().unwrap(), true);
        doc.font_face_loader.start_requests();
        assert!(doc.has_pending_font_loads());
        let (_, handler) = net.0.lock().unwrap().pop().unwrap();
        drop(handler);
        doc.handle_messages();
        doc.flush_pending_fonts();
        assert!(!doc.has_pending_font_loads());
        assert_eq!(
            doc.font_ctx
                .lock()
                .unwrap()
                .collection
                .pending_font_state(0),
            None
        );
    }
}
