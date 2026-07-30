use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::inspector::InspectorActor;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};

/// The highlighter actor renders a box-model overlay (content/padding/
/// border/margin) over a node when the user hovers it in the markup view.
/// It works by setting `DevtoolSettings::highlight_node` on the document
/// and requesting a redraw; blitz-paint then draws the overlay.
pub(crate) struct HighlighterActor {
    name: String,
    doc_id: usize,
    walker_name: String,
}

impl HighlighterActor {
    pub(crate) fn new(doc_id: usize, walker_name: String) -> Self {
        Self {
            name: generate_name("highlighter"),
            doc_id,
            walker_name,
        }
    }
}

impl Actor for HighlighterActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        let doc_id = self.doc_id;
        match &*message.type_ {
            "show" => {
                let msg = message.data.json()?;
                let node_actor = msg
                    .get("node")
                    .and_then(|v| v.as_str())
                    .ok_or(ActorMessageErr::MissingParameter)?
                    .to_string();
                // Tolerate non-node actors: Firefox passes the target/inspector
                // actor when showing e.g. the ViewportSizeOnResizeHighlighter,
                // and a noSuchNode error would abort its [root-node] listener
                // and leave the markup view empty.
                let node_id = InspectorActor::with_walker(ctx, &self.walker_name, |walker, _| {
                    walker.node_for_actor(&node_actor)
                });

                if let Some(node_id) = node_id {
                    ctx.with_doc(doc_id, |doc| {
                        // Text and comment nodes don't have a layout of their
                        // own: highlight the nearest element ancestor instead
                        let mut highlight_id = Some(node_id);
                        while let Some(id) = highlight_id {
                            let Some(node) = doc.get_node(id) else {
                                highlight_id = None;
                                break;
                            };
                            if node.element_data().is_some() {
                                break;
                            }
                            highlight_id = node.parent;
                        }
                        doc.devtools_mut().highlight_node = highlight_id;
                        doc.shell_provider.request_redraw();
                    });
                }
                ctx.write_msg(self.name(), json!({ "value": true }));
                Ok(())
            }
            "hide" => {
                ctx.with_doc(doc_id, |doc| {
                    doc.devtools_mut().highlight_node = None;
                    doc.shell_provider.request_redraw();
                });
                ctx.write_msg(self.name(), json!({}));
                Ok(())
            }
            "finalize" => {
                ctx.with_doc(doc_id, |doc| {
                    if doc.devtools().highlight_node.is_some() {
                        doc.devtools_mut().highlight_node = None;
                        doc.shell_provider.request_redraw();
                    }
                });
                // finalize is a one-way message: no reply
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
