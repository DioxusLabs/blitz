use blitz_dom::BaseDocument;
use blitz_traits::node_id::NodeId;
use serde_json::json;
use style::properties::{LonghandId, PropertyDeclarationId};
use taffy::DetailedGridTracksInfo;

use crate::actors::inspector::InspectorActor;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};
use crate::{GenericClientMessage, JsonValue};

/// The layout actor provides flexbox and grid inspection data.
pub(crate) struct LayoutActor {
    name: String,
    doc_id: usize,
    walker_name: String,
}

impl LayoutActor {
    pub(crate) fn new(doc_id: usize, walker_name: String) -> Self {
        Self {
            name: generate_name("layout"),
            doc_id,
            walker_name,
        }
    }
}

/// Serialize a computed style property to a string
fn str_prop(doc: &BaseDocument, node_id: NodeId, id: LonghandId) -> String {
    doc.get_node(node_id)
        .and_then(|node| node.primary_styles())
        .map(|styles| styles.computed_value_to_string(PropertyDeclarationId::Longhand(id)))
        .unwrap_or_default()
}

/// Whether a node's computed display is a flex container
fn is_flex_container(doc: &BaseDocument, node_id: NodeId) -> bool {
    str_prop(doc, node_id, LonghandId::Display).contains("flex")
}

/// Whether a node's computed display is a grid container
fn is_grid_container(doc: &BaseDocument, node_id: NodeId) -> bool {
    str_prop(doc, node_id, LonghandId::Display).contains("grid")
}

/// Find the flex container for a node: the node itself if it is a flex
/// container, otherwise its parent if that is one
fn find_flex_container(
    doc: &BaseDocument,
    node_id: NodeId,
    only_look_at_parents: bool,
) -> Option<NodeId> {
    if !only_look_at_parents && is_flex_container(doc, node_id) {
        return Some(node_id);
    }
    let parent_id = doc.get_node(node_id)?.parent?;
    is_flex_container(doc, parent_id).then_some(parent_id)
}

/// The `properties` object of a flex container form
fn flex_container_properties(doc: &BaseDocument, container_id: NodeId) -> JsonValue {
    json!({
        "align-content": str_prop(doc, container_id, LonghandId::AlignContent),
        "align-items": str_prop(doc, container_id, LonghandId::AlignItems),
        "flex-direction": str_prop(doc, container_id, LonghandId::FlexDirection),
        "flex-wrap": str_prop(doc, container_id, LonghandId::FlexWrap),
        "justify-content": str_prop(doc, container_id, LonghandId::JustifyContent),
    })
}

/// Serialize the tracks of one grid axis to devtools grid fragment format
fn tracks_form(tracks: &DetailedGridTracksInfo) -> JsonValue {
    let track_count = tracks.sizes.len();
    let explicit_start = tracks.negative_implicit_tracks as usize;
    let explicit_end = explicit_start + tracks.explicit_tracks as usize;
    let track_type = |idx: usize| -> &'static str {
        if idx >= explicit_start && idx < explicit_end {
            "explicit"
        } else {
            "implicit"
        }
    };

    let mut lines = Vec::with_capacity(track_count + 1);
    let mut track_forms = Vec::with_capacity(track_count);
    let mut position: f32 = 0.0;
    for (idx, size) in tracks.sizes.iter().enumerate() {
        // The gutter *before* track idx (gutters has len tracks + 1)
        let gutter_before = tracks.gutters.get(idx).copied().unwrap_or(0.0);
        position += gutter_before;

        lines.push(json!({
            "breadth": gutter_before,
            "names": [],
            "number": idx + 1,
            "start": position - gutter_before / 2.0,
            "type": track_type(idx),
        }));
        track_forms.push(json!({
            "breadth": size,
            "start": position,
            "state": "static",
            "type": track_type(idx),
        }));
        position += size;
    }
    let final_gutter = tracks.gutters.get(track_count).copied().unwrap_or(0.0);
    lines.push(json!({
        "breadth": final_gutter,
        "names": [],
        "number": track_count + 1,
        "start": position + final_gutter / 2.0,
        "type": track_type(track_count.saturating_sub(1)),
    }));

    json!({ "lines": lines, "tracks": track_forms })
}

/// Build the grid fragment for a grid container from the detailed grid info
/// stored during layout
fn grid_fragments(doc: &BaseDocument, container_id: NodeId) -> Option<JsonValue> {
    let node = doc.get_node(container_id)?;
    let grid_info = node.element_data()?.detailed_grid_info.as_ref()?;
    Some(json!([{
        "areas": [],
        "cols": tracks_form(&grid_info.columns),
        "rows": tracks_form(&grid_info.rows),
    }]))
}

/// Collect all grid containers in the document (in tree order)
fn collect_grid_containers(doc: &BaseDocument, node_id: NodeId, out: &mut Vec<NodeId>) {
    let Some(node) = doc.get_node(node_id) else {
        return;
    };
    if node.is_element() && is_grid_container(doc, node_id) {
        out.push(node_id);
    }
    for child_id in node.children.iter() {
        collect_grid_containers(doc, *child_id, out);
    }
}

impl Actor for LayoutActor {
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
            "getCurrentFlexbox" => {
                let msg = message.data.json()?;
                let node_actor = msg
                    .get("node")
                    .and_then(|v| v.as_str())
                    .ok_or(ActorMessageErr::MissingParameter)?
                    .to_string();
                let only_look_at_parents = msg
                    .get("onlyLookAtParents")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let node_id = InspectorActor::with_walker(ctx, &self.walker_name, |walker, _| {
                    walker.node_for_actor(&node_actor)
                })
                .ok_or(ActorMessageErr::NoSuchNode)?;

                let container = ctx
                    .with_doc(doc_id, |doc| {
                        find_flex_container(doc, node_id, only_look_at_parents)
                    })
                    .flatten();

                let Some(container_id) = container else {
                    ctx.write_msg(self.name(), json!({ "flexbox": null }));
                    return Ok(());
                };

                let flexbox = FlexboxActor::new(doc_id, self.walker_name.clone(), container_id);
                let flexbox_name = flexbox.name();
                ctx.push_actor(Box::new(flexbox));

                let container_actor =
                    InspectorActor::with_walker(ctx, &self.walker_name, |walker, _| {
                        walker.actor_for_node(container_id)
                    });
                let properties = ctx
                    .with_doc(doc_id, |doc| flex_container_properties(doc, container_id))
                    .ok_or(ActorMessageErr::NoSuchDocument)?;

                ctx.write_msg(
                    self.name(),
                    json!({ "flexbox": {
                        "actor": flexbox_name,
                        "containerNodeActorID": container_actor,
                        "properties": properties,
                    }}),
                );
                Ok(())
            }
            "getGrids" => {
                let container_ids = ctx
                    .with_doc(doc_id, |doc| {
                        let root_id = doc.root_node().id;
                        let mut out = Vec::new();
                        collect_grid_containers(doc, root_id, &mut out);
                        out
                    })
                    .ok_or(ActorMessageErr::NoSuchDocument)?;

                let mut grids = Vec::new();
                for container_id in container_ids {
                    let fragments = ctx
                        .with_doc(doc_id, |doc| grid_fragments(doc, container_id))
                        .flatten();
                    let Some(fragments) = fragments else {
                        continue;
                    };
                    let container_actor =
                        InspectorActor::with_walker(ctx, &self.walker_name, |walker, _| {
                            walker.actor_for_node(container_id)
                        });
                    let (direction, writing_mode) = ctx
                        .with_doc(doc_id, |doc| {
                            (
                                str_prop(doc, container_id, LonghandId::Direction),
                                str_prop(doc, container_id, LonghandId::WritingMode),
                            )
                        })
                        .unwrap_or_default();

                    let grid = GridActor::new(doc_id, container_id);
                    let grid_name = grid.name();
                    ctx.push_actor(Box::new(grid));

                    grids.push(json!({
                        "actor": grid_name,
                        "containerNodeActorID": container_actor,
                        "direction": direction,
                        "gridFragments": fragments,
                        "isSubgrid": false,
                        "writingMode": writing_mode,
                    }));
                }

                ctx.write_msg(self.name(), json!({ "grids": grids }));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}

/// A flexbox actor represents one flex container
pub(crate) struct FlexboxActor {
    name: String,
    doc_id: usize,
    walker_name: String,
    container_id: NodeId,
}

impl FlexboxActor {
    pub(crate) fn new(doc_id: usize, walker_name: String, container_id: NodeId) -> Self {
        Self {
            name: generate_name("flexbox"),
            doc_id,
            walker_name,
            container_id,
        }
    }
}

impl Actor for FlexboxActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        let doc_id = self.doc_id;
        let container_id = self.container_id;
        match &*message.type_ {
            "getFlexItems" => {
                // Gather the item data with document access
                struct ItemData {
                    node_id: NodeId,
                    main_base_size: f32,
                    main_min_size: f32,
                    main_max_size: JsonValue,
                    cross_min_size: f32,
                    cross_max_size: JsonValue,
                    main_axis_direction: String,
                    cross_axis_direction: String,
                    flex_grow: String,
                    flex_shrink: String,
                    flex_basis: String,
                }

                let items = ctx
                    .with_doc(doc_id, |doc| {
                        let container = doc.get_node(container_id)?;
                        let direction = str_prop(doc, container_id, LonghandId::FlexDirection);
                        let horizontal = !direction.starts_with("column");
                        let (main_axis_direction, cross_axis_direction) = if horizontal {
                            ("horizontal-lr".to_string(), "vertical-tb".to_string())
                        } else {
                            ("vertical-tb".to_string(), "horizontal-lr".to_string())
                        };

                        let mut items = Vec::new();
                        for child_id in container.children.iter().copied() {
                            let Some(child) = doc.get_node(child_id) else {
                                continue;
                            };
                            if !child.is_element() {
                                continue;
                            }
                            let layout = child.final_layout();
                            let main_size = if horizontal {
                                layout.size.width
                            } else {
                                layout.size.height
                            };
                            items.push(ItemData {
                                node_id: child_id,
                                main_base_size: main_size,
                                main_min_size: 0.0,
                                main_max_size: json!(null),
                                cross_min_size: 0.0,
                                cross_max_size: json!(null),
                                main_axis_direction: main_axis_direction.clone(),
                                cross_axis_direction: cross_axis_direction.clone(),
                                flex_grow: str_prop(doc, child_id, LonghandId::FlexGrow),
                                flex_shrink: str_prop(doc, child_id, LonghandId::FlexShrink),
                                flex_basis: str_prop(doc, child_id, LonghandId::FlexBasis),
                            });
                        }
                        Some(items)
                    })
                    .flatten()
                    .ok_or(ActorMessageErr::NoSuchNode)?;

                let mut item_forms = Vec::with_capacity(items.len());
                for item in items {
                    let node_actor =
                        InspectorActor::with_walker(ctx, &self.walker_name, |walker, _| {
                            walker.actor_for_node(item.node_id)
                        });
                    let item_actor = crate::actors::stubs::StubActor::new("flex-item");
                    let item_actor_name = item_actor.name();
                    ctx.push_actor(Box::new(item_actor));

                    item_forms.push(json!({
                        "actor": item_actor_name,
                        "nodeActorID": node_actor,
                        "flexItemSizing": {
                            "crossAxisDirection": item.cross_axis_direction,
                            "mainAxisDirection": item.main_axis_direction,
                            "crossMaxSize": item.cross_max_size,
                            "crossMinSize": item.cross_min_size,
                            "mainBaseSize": item.main_base_size,
                            "mainDeltaSize": 0,
                            "mainMaxSize": item.main_max_size,
                            "mainMinSize": item.main_min_size,
                            "lineGrowthState": "growing",
                            "clampState": "unclamped",
                        },
                        "properties": {
                            "flex-basis": item.flex_basis,
                            "flex-grow": item.flex_grow,
                            "flex-shrink": item.flex_shrink,
                        },
                        "computedStyle": {
                            "flexGrow": item.flex_grow,
                            "flexShrink": item.flex_shrink,
                        },
                    }));
                }

                ctx.write_msg(self.name(), json!({ "flexItems": item_forms }));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}

/// A grid actor represents one grid container
pub(crate) struct GridActor {
    name: String,
    #[allow(dead_code)]
    doc_id: usize,
    #[allow(dead_code)]
    container_id: NodeId,
}

impl GridActor {
    pub(crate) fn new(doc_id: usize, container_id: NodeId) -> Self {
        Self {
            name: generate_name("grid"),
            doc_id,
            container_id,
        }
    }
}

impl Actor for GridActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        _message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        ctx.write_msg(self.name(), json!({}));
        Ok(())
    }
}
