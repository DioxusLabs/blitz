use std::collections::HashMap;

use blitz_dom::BaseDocument;
use blitz_traits::node_id::NodeId;
use cssparser::ToCss as _;
use serde_json::json;
use style::properties::{LonghandId, NonCustomPropertyId, PropertyDeclarationId, PropertyId};
use style::rule_tree::CascadeOrigin;
use style::stylesheets::{CssRule, Origin};

use crate::actors::inspector::InspectorActor;
use crate::actors::stubs::StubActor;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};
use crate::{GenericClientMessage, JsonValue};

/// Rule type constants from the CSSOM spec (and Firefox devtools)
const STYLE_RULE: u32 = 1;
/// Pseudo rule type used by Firefox devtools for element inline styles
const ELEMENT_RULE: u32 = 100;

/// The page style actor provides the data for the style inspector panels:
/// box model layout, computed styles, and matched ("applied") style rules.
pub(crate) struct PageStyleActor {
    name: String,
    doc_id: usize,
    walker_name: String,
}

/// Metadata about the style rule that a declaration block came from,
/// obtained by walking the document's stylesheets
struct RuleMetadata {
    selector_text: String,
    href: String,
    origin: Origin,
    line: u32,
    column: u32,
}

impl PageStyleActor {
    pub(crate) fn new(doc_id: usize, walker_name: String) -> Self {
        Self {
            name: generate_name("page-style"),
            doc_id,
            walker_name,
        }
    }

    /// Build a map from declaration-block pointer to rule metadata by
    /// walking all of the document's stylesheets
    fn rule_metadata(doc: &BaseDocument) -> HashMap<*const std::ffi::c_void, RuleMetadata> {
        let guard_holder = doc.guard();
        let guard = guard_holder.read();

        let mut map = HashMap::new();

        fn visit_rules(
            rules: &[CssRule],
            guard: &style::shared_lock::SharedRwLockReadGuard,
            href: &str,
            origin: Origin,
            map: &mut HashMap<*const std::ffi::c_void, RuleMetadata>,
        ) {
            for rule in rules {
                match rule {
                    CssRule::Style(locked_rule) => {
                        let style_rule = locked_rule.read_with(guard);
                        let key = style_rule.block.heap_ptr();
                        map.insert(
                            key,
                            RuleMetadata {
                                selector_text: style_rule.selectors.to_css_string(),
                                href: href.to_string(),
                                origin,
                                line: style_rule.source_location.line,
                                column: style_rule.source_location.column,
                            },
                        );
                        if let Some(nested) = &style_rule.rules {
                            visit_rules(&nested.read_with(guard).0, guard, href, origin, map);
                        }
                    }
                    CssRule::Media(media_rule) => {
                        visit_rules(
                            &media_rule.rules.read_with(guard).0,
                            guard,
                            href,
                            origin,
                            map,
                        );
                    }
                    CssRule::Supports(supports_rule) => {
                        visit_rules(
                            &supports_rule.rules.read_with(guard).0,
                            guard,
                            href,
                            origin,
                            map,
                        );
                    }
                    _ => {}
                }
            }
        }

        for (href, origin, sheet) in doc
            .author_stylesheets()
            .map(|sheet| ("", Origin::Author, sheet))
            .chain(
                doc.useragent_stylesheets()
                    .map(|sheet| ("ua.css", Origin::UserAgent, sheet)),
            )
        {
            let contents = sheet.0.contents.read_with(&guard);
            let rules = contents.rules.read_with(&guard);
            visit_rules(&rules.0, &guard, href, origin, &mut map);
        }

        map
    }
}

impl Actor for PageStyleActor {
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
            "getLayout" => {
                let node_id = resolve_node(ctx, &self.walker_name, message.data.json()?)?;
                let layout = ctx
                    .with_doc(doc_id, |doc| layout_form(doc, node_id))
                    .flatten()
                    .ok_or(ActorMessageErr::NoSuchNode)?;
                ctx.write_msg(self.name(), layout);
                Ok(())
            }
            "getComputed" => {
                let node_id = resolve_node(ctx, &self.walker_name, message.data.json()?)?;
                let computed = ctx
                    .with_doc(doc_id, |doc| computed_form(doc, node_id))
                    .flatten()
                    .ok_or(ActorMessageErr::NoSuchNode)?;
                ctx.write_msg(self.name(), json!({ "computed": computed }));
                Ok(())
            }
            "getApplied" => {
                let msg = message.data.json()?;
                let inherited = msg
                    .get("inherited")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let node_id = resolve_node(ctx, &self.walker_name, msg)?;

                // Collect the matched rules (and the node ids they're
                // inherited from) with document access
                let entries = ctx
                    .with_doc(doc_id, |doc| applied_entries(doc, node_id, inherited))
                    .ok_or(ActorMessageErr::NoSuchDocument)?;

                // Then resolve node ids to node actor names via the walker,
                // registering rule actor stubs as we go
                let mut json_entries = Vec::with_capacity(entries.len());
                for entry in entries {
                    let rule_actor = StubActor::new("style-rule");
                    let rule_actor_name = rule_actor.name();
                    ctx.push_actor(Box::new(rule_actor));

                    let inherited_form = match entry.inherited_from {
                        Some(ancestor_id) => {
                            let form = InspectorActor::with_walker(
                                ctx,
                                &self.walker_name,
                                |walker, ctx| {
                                    ctx.with_doc(doc_id, |doc| walker.node_form(doc, ancestor_id))
                                },
                            );
                            form.flatten().unwrap_or(JsonValue::Null)
                        }
                        None => JsonValue::Null,
                    };

                    let declarations: Vec<JsonValue> = entry
                        .declarations
                        .iter()
                        .map(|decl| {
                            json!({
                                "name": decl.name,
                                "value": decl.value,
                                "priority": if decl.important { "important" } else { "" },
                                "important": decl.important,
                                "isUsed": { "used": true },
                                "terminator": "",
                            })
                        })
                        .collect();

                    let css_text: String = entry
                        .declarations
                        .iter()
                        .map(|decl| {
                            if decl.important {
                                format!("{}: {} !important; ", decl.name, decl.value)
                            } else {
                                format!("{}: {}; ", decl.name, decl.value)
                            }
                        })
                        .collect();

                    // An empty selector string makes Firefox's rule-editor
                    // throw in parsePseudoClassesAndAttributes, leaving the
                    // Rules panel empty. Fall back to "*".
                    let selector_text = if entry.selector_text.is_empty() {
                        "*".to_string()
                    } else {
                        entry.selector_text.clone()
                    };
                    json_entries.push(json!({
                        "rule": {
                            "actor": rule_actor_name,
                            "type": entry.rule_type,
                            "href": entry.href,
                            "cssText": css_text,
                            "authoredText": format!("{} {{ {} }}", selector_text, css_text),
                            "selectors": [selector_text],
                            "selectorsSpecificity": [entry.specificity],
                            "line": entry.line,
                            "column": entry.column,
                            "declarations": declarations,
                            // Firefox's rule-editor requires ancestorData;
                            // without it the Rules panel throws and renders
                            // empty
                            "ancestorData": [],
                            "traits": { "canSetRuleText": false },
                        },
                        "pseudoElement": null,
                        "isSystem": entry.is_system,
                        "inherited": inherited_form,
                    }));
                }

                ctx.write_msg(self.name(), json!({ "entries": json_entries }));
                Ok(())
            }
            "isPositionEditable" => {
                ctx.write_msg(self.name(), json!({ "value": false }));
                Ok(())
            }
            // Firefox's Rules view calls getUsedFontFaces when populating;
            // an unrecognizedPacketType error aborts the populate and leaves
            // the Rules panel empty
            "getUsedFontFaces" => {
                ctx.write_msg(self.name(), json!({ "fontFaces": [] }));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}

/// Resolve the `node` parameter of a message to a `NodeId` via the walker
fn resolve_node(
    ctx: &mut DevtoolContext<'_>,
    walker_name: &str,
    msg: &JsonValue,
) -> Result<NodeId, ActorMessageErr> {
    let node_actor = msg
        .get("node")
        .and_then(|v| v.as_str())
        .ok_or(ActorMessageErr::MissingParameter)?
        .to_string();
    InspectorActor::with_walker(ctx, walker_name, |walker, _| {
        walker.node_for_actor(&node_actor)
    })
    .ok_or(ActorMessageErr::NoSuchNode)
}

/// Serialize an f32 as a CSS pixel string
fn px(value: f32) -> String {
    format!("{value}px")
}

/// Build the `getLayout` (box model) response for a node
fn layout_form(doc: &BaseDocument, node_id: NodeId) -> Option<JsonValue> {
    let node = doc.get_node(node_id)?;
    // Text and comment nodes don't carry a layout of their own
    node.element_data()?;
    let layout = node.final_layout();

    let styles = node.primary_styles();
    let str_prop = |id: LonghandId| -> String {
        styles
            .as_ref()
            .map(|s| s.computed_value_to_string(PropertyDeclarationId::Longhand(id)))
            .unwrap_or_default()
    };

    let mut auto_margins = serde_json::Map::new();
    if let Some(styles) = styles.as_ref() {
        let margin = styles.get_margin();
        if margin.margin_top.is_auto() {
            auto_margins.insert("top".to_string(), json!("auto"));
        }
        if margin.margin_right.is_auto() {
            auto_margins.insert("right".to_string(), json!("auto"));
        }
        if margin.margin_bottom.is_auto() {
            auto_margins.insert("bottom".to_string(), json!("auto"));
        }
        if margin.margin_left.is_auto() {
            auto_margins.insert("left".to_string(), json!("auto"));
        }
    }

    Some(json!({
        "width": layout.size.width - layout.border.left - layout.border.right
            - layout.padding.left - layout.padding.right,
        "height": layout.size.height - layout.border.top - layout.border.bottom
            - layout.padding.top - layout.padding.bottom,
        "autoMargins": auto_margins,
        "margin-top": px(layout.margin.top),
        "margin-right": px(layout.margin.right),
        "margin-bottom": px(layout.margin.bottom),
        "margin-left": px(layout.margin.left),
        "border-top-width": px(layout.border.top),
        "border-right-width": px(layout.border.right),
        "border-bottom-width": px(layout.border.bottom),
        "border-left-width": px(layout.border.left),
        "padding-top": px(layout.padding.top),
        "padding-right": px(layout.padding.right),
        "padding-bottom": px(layout.padding.bottom),
        "padding-left": px(layout.padding.left),
        "display": str_prop(LonghandId::Display),
        "float": str_prop(LonghandId::Float),
        "line-height": str_prop(LonghandId::LineHeight),
        "position": str_prop(LonghandId::Position),
        "z-index": str_prop(LonghandId::ZIndex),
        "box-sizing": str_prop(LonghandId::BoxSizing),
    }))
}

/// Build the `getComputed` response for a node: every longhand property
/// serialized from the node's computed style
fn computed_form(doc: &BaseDocument, node_id: NodeId) -> Option<JsonValue> {
    let node = doc.get_node(node_id)?;
    let styles = node.primary_styles()?;

    let mut computed = serde_json::Map::new();
    for id in NonCustomPropertyId::iter() {
        let Ok(longhand) = id.longhand_or_shorthand() else {
            continue;
        };
        if id.as_alias().is_some() {
            continue;
        }
        if !PropertyId::NonCustom(id).enabled_for_all_content() {
            continue;
        }
        let value = styles.computed_value_to_string(PropertyDeclarationId::Longhand(longhand));
        computed.insert(
            longhand.name().to_string(),
            json!({ "matched": true, "value": value }),
        );
    }
    Some(JsonValue::Object(computed))
}

/// A single declaration in an applied rule
struct AppliedDeclaration {
    name: String,
    value: String,
    important: bool,
}

/// A single entry in the `getApplied` response, gathered with document
/// access and serialized to JSON afterwards
struct AppliedEntry {
    rule_type: u32,
    selector_text: String,
    href: String,
    line: u32,
    column: u32,
    specificity: u32,
    is_system: bool,
    declarations: Vec<AppliedDeclaration>,
    inherited_from: Option<NodeId>,
}

/// Collect the applied style rules for a node (and optionally the inherited
/// rules from its ancestors) by walking Stylo's rule tree
fn applied_entries(doc: &BaseDocument, node_id: NodeId, inherited: bool) -> Vec<AppliedEntry> {
    let metadata = PageStyleActor::rule_metadata(doc);
    let guard_holder = doc.guard();
    let guard = guard_holder.read();

    let mut entries = Vec::new();

    let mut current = Some(node_id);
    while let Some(current_id) = current {
        let Some(node) = doc.get_node(current_id) else {
            break;
        };
        let inherited_from = (current_id != node_id).then_some(current_id);

        // The inline style attribute (only for the node itself)
        if inherited_from.is_none()
            && let Some(element) = node.element_data()
            && let Some(style_attr) = &element.style_attribute
        {
            let block = style_attr.read_with(&guard);
            let declarations = block_declarations(block, false);
            if !declarations.is_empty() {
                entries.push(AppliedEntry {
                    rule_type: ELEMENT_RULE,
                    selector_text: "element".to_string(),
                    href: String::new(),
                    line: 0,
                    column: 0,
                    specificity: 0,
                    is_system: false,
                    declarations,
                    inherited_from: None,
                });
            }
        }

        if let Some(styles) = node.primary_styles()
            && let Some(rules) = &styles.rules
        {
            for rule_node in rules.self_and_ancestors() {
                let Some(source) = rule_node.style_source() else {
                    continue;
                };
                let block = source.read(&guard);

                // Skip the inline style attribute (handled above)
                let is_style_attribute = node
                    .element_data()
                    .and_then(|element| element.style_attribute.as_ref())
                    .is_some_and(|style_attr| {
                        style::servo_arc::Arc::ptr_eq(source.get(), style_attr)
                    });
                if is_style_attribute {
                    continue;
                }

                // For inherited entries, only include rules with at
                // least one inherited property
                let inherited_only = inherited_from.is_some();
                let declarations = block_declarations(block, inherited_only);
                if declarations.is_empty() {
                    continue;
                }

                let origin_is_ua = rule_node.cascade_level().origin() == CascadeOrigin::UA;
                let key = source.get().heap_ptr();
                let meta = metadata.get(&key);

                entries.push(AppliedEntry {
                    rule_type: STYLE_RULE,
                    selector_text: meta.map(|m| m.selector_text.clone()).unwrap_or_default(),
                    href: meta.map(|m| m.href.clone()).unwrap_or_default(),
                    line: meta.map(|m| m.line).unwrap_or(0),
                    column: meta.map(|m| m.column).unwrap_or(0),
                    specificity: 0,
                    is_system: meta
                        .map(|m| m.origin == Origin::UserAgent)
                        .unwrap_or(origin_is_ua),
                    declarations,
                    inherited_from,
                });
            }
        }

        if !inherited {
            break;
        }
        current = node.parent;
    }

    entries
}

/// Extract the declarations from a declaration block, optionally keeping
/// only inherited properties
fn block_declarations(
    block: &style::properties::PropertyDeclarationBlock,
    inherited_only: bool,
) -> Vec<AppliedDeclaration> {
    block
        .declaration_importance_iter()
        .filter_map(|(decl, importance)| {
            let id = decl.id();
            if inherited_only {
                let is_inherited = match id {
                    PropertyDeclarationId::Longhand(longhand) => longhand.inherited(),
                    PropertyDeclarationId::Custom(_) => true,
                };
                if !is_inherited {
                    return None;
                }
            }
            let mut value = String::new();
            decl.to_css(&mut value).ok()?;
            Some(AppliedDeclaration {
                name: id.name().to_string(),
                value,
                important: importance.important(),
            })
        })
        .collect()
}
