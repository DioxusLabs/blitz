use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::highlighter::HighlighterActor;
use crate::actors::page_style::PageStyleActor;
use crate::actors::walker::WalkerActor;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};

/// The inspector actor is the entry point to the DOM/style inspection
/// actors: the walker (DOM tree), page style (style panels), and
/// highlighters.
pub(crate) struct InspectorActor {
    name: String,
    doc_id: usize,
    walker_name: Option<String>,
    page_style_name: Option<String>,
}

impl InspectorActor {
    pub(crate) fn new(doc_id: usize) -> Self {
        Self {
            name: generate_name("inspector"),
            doc_id,
            walker_name: None,
            page_style_name: None,
        }
    }

    fn ensure_walker(&mut self, ctx: &mut DevtoolContext<'_>) -> String {
        if let Some(name) = &self.walker_name {
            return name.clone();
        }
        let walker = WalkerActor::new(self.doc_id);
        let name = walker.name();
        self.walker_name = Some(name.clone());
        ctx.push_actor(Box::new(walker));
        name
    }

    /// Run a callback with mutable access to the walker actor, which may
    /// either be already registered or newly created (pending registration)
    pub(crate) fn with_walker<R>(
        ctx: &mut DevtoolContext<'_>,
        walker_name: &str,
        cb: impl FnOnce(&mut WalkerActor, &mut DevtoolContext<'_>) -> R,
    ) -> R {
        if let Some(idx) = ctx
            .actors_to_create
            .iter()
            .position(|actor| actor.name() == walker_name)
        {
            let mut walker = ctx.actors_to_create.remove(idx);
            let walker_ref = (walker.as_mut() as &mut dyn std::any::Any)
                .downcast_mut::<WalkerActor>()
                .unwrap();
            let result = cb(walker_ref, ctx);
            ctx.actors_to_create.push(walker);
            result
        } else {
            let mut walker = ctx.actors.remove(walker_name).unwrap();
            let walker_ref = (walker.as_mut() as &mut dyn std::any::Any)
                .downcast_mut::<WalkerActor>()
                .unwrap();
            let result = cb(walker_ref, ctx);
            ctx.actors.insert(walker_name.to_string(), walker);
            result
        }
    }
}

impl Actor for InspectorActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "getWalker" => {
                let walker_name = self.ensure_walker(ctx);
                let root =
                    Self::with_walker(ctx, &walker_name, |walker, ctx| walker.root_form(ctx));
                let root = root.ok_or(ActorMessageErr::NoSuchDocument)?;

                ctx.write_msg(
                    self.name(),
                    json!({ "walker": {
                        "actor": walker_name,
                        "root": root,
                    }}),
                );
                Ok(())
            }
            "getPageStyle" => {
                let page_style_name = match &self.page_style_name {
                    Some(name) => name.clone(),
                    None => {
                        let walker_name = self.ensure_walker(ctx);
                        let page_style = PageStyleActor::new(self.doc_id, walker_name);
                        let name = page_style.name();
                        self.page_style_name = Some(name.clone());
                        ctx.push_actor(Box::new(page_style));
                        name
                    }
                };
                ctx.write_msg(
                    self.name(),
                    json!({ "pageStyle": {
                        "actor": page_style_name,
                        "traits": {},
                    }}),
                );
                Ok(())
            }
            "getHighlighterByType" => {
                let walker_name = self.ensure_walker(ctx);
                let highlighter = HighlighterActor::new(self.doc_id, walker_name);
                let name = highlighter.name();
                ctx.push_actor(Box::new(highlighter));
                ctx.write_msg(self.name(), json!({ "highlighter": { "actor": name } }));
                Ok(())
            }
            "supportsHighlighters" => {
                ctx.write_msg(self.name(), json!({ "value": true }));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
