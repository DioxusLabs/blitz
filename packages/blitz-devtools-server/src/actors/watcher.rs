use serde_json::json;

use crate::actors::css_properties::CssPropertiesActor;
use crate::actors::inspector::InspectorActor;
use crate::actors::stubs::{ConsoleActor, StubActor, ThreadActor};
use crate::actors::tab::doc_title;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};
use crate::{GenericClientMessage, JsonValue};

/// The actor names for the session actors associated with a single tab/frame target
#[derive(Clone)]
pub(crate) struct SessionNames {
    pub frame_target: String,
    pub inspector: String,
    pub css_properties: String,
    pub console: String,
    pub thread: String,
    pub target_configuration: String,
    pub thread_configuration: String,
    pub network_parent: String,
    pub breakpoint_list: String,
    pub blackboxing: String,
    pub reflow: String,
    pub style_sheets: String,
    pub accessibility: String,
}

/// The watcher actor describes the debugging capabilities of a tab and hands
/// out the (frame) target when the client starts watching.
pub(crate) struct WatcherActor {
    name: String,
    doc_id: usize,
    pub(crate) session: SessionNames,
}

impl WatcherActor {
    /// Create the watcher along with all of its session actors, registering
    /// them with the connection
    pub(crate) fn create(ctx: &mut DevtoolContext<'_>, doc_id: usize) -> WatcherActor {
        let inspector = InspectorActor::new(doc_id);
        let css_properties = CssPropertiesActor::new();
        let console = ConsoleActor::new();
        let thread = ThreadActor::new();
        let target_configuration = StubActor::new("target-configuration");
        let thread_configuration = StubActor::new("thread-configuration");
        let network_parent = StubActor::new("network-parent");
        let breakpoint_list = StubActor::new("breakpoint-list");
        let blackboxing = StubActor::new("blackboxing");
        let reflow = StubActor::new("reflow");
        let style_sheets = StubActor::new("style-sheets");
        let accessibility = StubActor::new("accessibility");
        let frame_target = StubActor::new("frame-target");

        let session = SessionNames {
            frame_target: frame_target.name(),
            inspector: inspector.name(),
            css_properties: css_properties.name(),
            console: console.name(),
            thread: thread.name(),
            target_configuration: target_configuration.name(),
            thread_configuration: thread_configuration.name(),
            network_parent: network_parent.name(),
            breakpoint_list: breakpoint_list.name(),
            blackboxing: blackboxing.name(),
            reflow: reflow.name(),
            style_sheets: style_sheets.name(),
            accessibility: accessibility.name(),
        };

        ctx.push_actor(Box::new(inspector));
        ctx.push_actor(Box::new(css_properties));
        ctx.push_actor(Box::new(console));
        ctx.push_actor(Box::new(thread));
        ctx.push_actor(Box::new(target_configuration));
        ctx.push_actor(Box::new(thread_configuration));
        ctx.push_actor(Box::new(network_parent));
        ctx.push_actor(Box::new(breakpoint_list));
        ctx.push_actor(Box::new(blackboxing));
        ctx.push_actor(Box::new(reflow));
        ctx.push_actor(Box::new(style_sheets));
        ctx.push_actor(Box::new(accessibility));
        ctx.push_actor(Box::new(frame_target));

        WatcherActor {
            name: generate_name("watcher"),
            doc_id,
            session,
        }
    }

    /// Build the "frame target" form describing the browsing context target
    /// and its supporting actors
    pub(crate) fn frame_target_form(
        ctx: &mut DevtoolContext<'_>,
        doc_id: usize,
        session: &SessionNames,
    ) -> Option<JsonValue> {
        let session = session.clone();
        ctx.with_doc(doc_id, |doc| {
            json!({
                "actor": session.frame_target,
                "title": doc_title(doc),
                "url": doc.url().to_string(),
                "browserId": doc_id,
                "outerWindowID": doc_id,
                "browsingContextID": doc_id,
                "isTopLevelTarget": true,
                "targetType": "frame",
                // We have no subframes; this tells the client not to query
                // them (the `frames` trait must still be true, as the toolbox
                // gates the element picker button on it)
                "ignoreSubFrames": true,
                "traits": {
                    "frames": true,
                    "isBrowsingContext": true,
                    "logInPage": false,
                    "navigation": false,
                    "supportsTopLevelTargetFlag": true,
                    "watchpoints": false,
                },
                "accessibilityActor": session.accessibility,
                "consoleActor": session.console,
                "cssPropertiesActor": session.css_properties,
                "inspectorActor": session.inspector,
                "reflowActor": session.reflow,
                "styleSheetsActor": session.style_sheets,
                "threadActor": session.thread,
            })
        })
    }
}

impl Actor for WatcherActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "watchTargets" => {
                let msg = message.data.json()?;
                let target_type = msg
                    .get("targetType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("frame");

                if target_type == "frame" {
                    let target = Self::frame_target_form(ctx, self.doc_id, &self.session)
                        .ok_or(ActorMessageErr::NoSuchDocument)?;
                    ctx.write_msg(
                        self.name(),
                        json!({ "type": "target-available-form", "target": target }),
                    );
                }

                // Notifications (messages with a `type` field) don't count as
                // a reply, so send an empty reply to finish the request
                ctx.write_msg(self.name(), json!({}));
                Ok(())
            }
            // One-way messages that expect no reply
            "unwatchTargets" | "unwatchResources" => Ok(()),
            "watchResources" => {
                ctx.write_msg(self.name(), json!({}));
                Ok(())
            }
            "getParentBrowsingContextID" => {
                ctx.write_msg(self.name(), json!({ "browsingContextID": self.doc_id }));
                Ok(())
            }
            "getNetworkParentActor" => {
                ctx.write_msg(
                    self.name(),
                    json!({ "network": { "actor": self.session.network_parent } }),
                );
                Ok(())
            }
            "getTargetConfigurationActor" => {
                ctx.write_msg(
                    self.name(),
                    json!({ "configuration": {
                        "actor": self.session.target_configuration,
                        "configuration": {},
                        "traits": { "supportedOptions": {} },
                    }}),
                );
                Ok(())
            }
            "getThreadConfigurationActor" => {
                ctx.write_msg(
                    self.name(),
                    json!({ "configuration": { "actor": self.session.thread_configuration } }),
                );
                Ok(())
            }
            "getBreakpointListActor" => {
                ctx.write_msg(
                    self.name(),
                    json!({ "breakpointList": { "actor": self.session.breakpoint_list } }),
                );
                Ok(())
            }
            "getBlackboxingActor" => {
                ctx.write_msg(
                    self.name(),
                    json!({ "blackboxing": { "actor": self.session.blackboxing } }),
                );
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}
