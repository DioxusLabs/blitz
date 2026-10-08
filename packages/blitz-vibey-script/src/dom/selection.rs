//! Selection API bindings and DOM-to-rendered text offset mapping.

use std::cell::Cell;

use blitz_dom::text::InlineText as _;
use blitz_dom::{BaseDocument, Node, NodeId, node::NodeData};
use boa_engine::{
    Context, Finalize, JsData, JsNativeError, JsResult, JsValue, Trace, object::JsObject,
};
use icu_properties::{
    CodePointMapData,
    props::{GeneralCategory, GeneralCategoryGroup},
};
use style::properties::longhands::white_space_collapse::computed_value::T as WhiteSpaceCollapse;
use style::values::computed::TextTransform;

use super::{define_method, dom_ctx, dom_exception, js_str, node_id_of_value};

type Point = (NodeId, usize);
type Range = (NodeId, Option<usize>, usize, usize);

#[derive(Trace, Finalize, JsData)]
struct Selection {
    #[unsafe_ignore_trace]
    points: Cell<Option<[Point; 2]>>,
    #[unsafe_ignore_trace]
    rendered_range: Cell<Option<[Range; 2]>>,
}

pub(crate) fn get_selection(
    _: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    if let Some(selection) = &ctx.state.borrow().selection {
        return Ok(selection.clone().into());
    }
    let selection = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        Selection {
            points: Cell::new(None),
            rendered_range: Cell::new(None),
        },
    );
    define_method(
        &selection,
        "setBaseAndExtent",
        4,
        set_base_and_extent,
        context,
    );
    define_method(&selection, "toString", 0, to_string, context);
    define_method(&selection, "removeAllRanges", 0, remove_all_ranges, context);
    define_method(&selection, "empty", 0, remove_all_ranges, context);
    ctx.state.borrow_mut().selection = Some(selection.clone());
    Ok(selection.into())
}

fn selection_object(this: &JsValue) -> JsResult<JsObject> {
    this.as_object()
        .filter(|object| object.is::<Selection>())
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("`this` is not a Selection")
                .into()
        })
}

fn set_base_and_extent(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let selection = selection_object(this)?;
    if args.len() < 4 {
        return Err(JsNativeError::typ()
            .with_message("setBaseAndExtent requires four arguments")
            .into());
    }
    let anchor = node_id_of_value(&args[0])
        .ok_or_else(|| JsNativeError::typ().with_message("anchorNode is not a Node"))?;
    let focus = node_id_of_value(&args[2])
        .ok_or_else(|| JsNativeError::typ().with_message("focusNode is not a Node"))?;
    let points = [
        (anchor, args[1].to_u32(context)? as usize),
        (focus, args[3].to_u32(context)? as usize),
    ];
    let ctx = dom_ctx(context)?;
    let mut doc = ctx.doc.borrow_mut();
    for &(id, offset) in &points {
        let Some(node) = doc.get_node(id) else {
            return Ok(JsValue::undefined());
        };
        let length = match &node.data {
            NodeData::Text(text) => text.content.encode_utf16().count(),
            NodeData::Comment { contents } => contents.encode_utf16().count(),
            _ => node.children.len(),
        };
        if offset > length {
            return Err(dom_exception(
                context,
                "IndexSizeError",
                "Selection offset exceeds the node's length",
            ));
        }
    }
    if points.iter().any(|&(id, _)| {
        !doc.get_node(id)
            .is_some_and(|node| node.flags.is_in_document())
    }) {
        return Ok(JsValue::undefined());
    }
    doc.resolve(0.0);
    apply_points(&mut doc, points);
    let data = selection.downcast_ref::<Selection>().unwrap();
    data.points.set(Some(points));
    data.rendered_range.set(range_endpoints(&doc));
    Ok(JsValue::undefined())
}

fn remove_all_ranges(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let object = selection_object(this)?;
    let data = object.downcast_ref::<Selection>().unwrap();
    data.points.set(None);
    data.rendered_range.set(None);
    dom_ctx(context)?.doc.borrow_mut().clear_text_selection();
    Ok(JsValue::undefined())
}

fn to_string(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let object = selection_object(this)?;
    let data = object.downcast_ref::<Selection>().unwrap();
    let ctx = dom_ctx(context)?;
    let mut doc = ctx.doc.borrow_mut();
    // A pointer selection replaces a selection previously made by script.
    if range_endpoints(&doc) != data.rendered_range.get() {
        data.points.set(None);
    }
    doc.resolve(0.0);
    if let Some(points) = data.points.get() {
        apply_points(&mut doc, points);
        data.rendered_range.set(range_endpoints(&doc));
    }
    Ok(js_str(&doc.get_selected_text().unwrap_or_default()))
}

fn range_endpoints(doc: &BaseDocument) -> Option<[Range; 2]> {
    let ranges = doc.get_text_selection_ranges();
    let stable_range = |&(id, start, end): &(NodeId, usize, usize)| {
        let node = doc.get_node(id)?;
        // Anonymous inline roots are rebuilt, so compare their stable location.
        let (id, index) = if node.is_anonymous() {
            let parent = doc.get_node(node.parent?)?;
            let children = parent.layout_children.borrow();
            let index = children
                .as_ref()?
                .iter()
                .take_while(|&&child| child != node.id)
                .filter(|&&child| doc.get_node(child).is_some_and(Node::is_anonymous))
                .count();
            (parent.id, Some(index))
        } else {
            (id, None)
        };
        Some((id, index, start, end))
    };
    Some([
        stable_range(ranges.first()?)?,
        stable_range(ranges.last()?)?,
    ])
}

fn apply_points(doc: &mut BaseDocument, points: [Point; 2]) {
    match (
        rendered_point(doc, points[0]),
        rendered_point(doc, points[1]),
    ) {
        (Some((anchor, start)), Some((focus, end))) => {
            doc.set_text_selection(anchor, start, focus, end)
        }
        _ => doc.clear_text_selection(),
    }
}

fn rendered_point(doc: &BaseDocument, point: Point) -> Option<Point> {
    let node = doc.get_node(point.0)?;
    if !node.flags.is_in_document() {
        return None;
    }
    if let Some(root) = inline_root(doc, node.id) {
        let layout = doc
            .get_node(root)?
            .element_data()?
            .inline_layout_data
            .as_ref()?;
        if layout.maps_source() {
            // The layout maps a text node's own offsets; a point between nodes takes the
            // adjacent text's, below.
            if let NodeData::Text(data) = &node.data {
                let offset = utf16_to_byte_offset(&data.content, point.1);
                return layout
                    .source_offset(node.id, offset)
                    .map(|offset| (root, offset));
            }
            // A point in a node with neither text nor children, such as a comment, stands
            // where the node does.
            if node.children.is_empty() {
                if let Some(parent) = node.parent.and_then(|id| doc.get_node(id)) {
                    let index = parent.children.iter().position(|&id| id == node.id)?;
                    return rendered_point(doc, (parent.id, index + 1));
                }
            }
        } else {
            let mut mapper = OffsetMapper {
                doc,
                root,
                text: layout.text(),
                cursor: 0,
                collapsed_space: false,
                point,
                result: None,
            };
            mapper.visit(root);
            if let Some(offset) = mapper.result {
                return Some((root, offset));
            }
        }
    }

    // Block containers have no inline layout of their own. Use the adjacent text.
    let offset = point.1.min(node.children.len());
    node.children[offset..]
        .iter()
        .find_map(|&id| subtree_point(doc, id, false))
        .or_else(|| {
            node.children[..offset]
                .iter()
                .rev()
                .find_map(|&id| subtree_point(doc, id, true))
        })
}

/// The byte offset in `text` of UTF-16 offset `units`, clamped to its end.
fn utf16_to_byte_offset(text: &str, units: usize) -> usize {
    let mut count = 0;
    for (index, c) in text.char_indices() {
        if count >= units {
            return index;
        }
        count += c.len_utf16();
    }
    text.len()
}

fn inline_root(doc: &BaseDocument, mut id: NodeId) -> Option<NodeId> {
    loop {
        let node = doc.get_node(id)?;
        if node
            .element_data()
            .is_some_and(|element| element.inline_layout_data.is_some())
        {
            return Some(id);
        }
        id = node.layout_parent.get()?;
    }
}

fn subtree_point(doc: &BaseDocument, id: NodeId, end: bool) -> Option<Point> {
    let node = doc.get_node(id)?;
    if let NodeData::Text(data) = &node.data {
        let offset = if end {
            data.content.encode_utf16().count()
        } else {
            0
        };
        return rendered_point(doc, (id, offset));
    }
    if end {
        node.children
            .iter()
            .rev()
            .find_map(|&id| subtree_point(doc, id, end))
    } else {
        node.children
            .iter()
            .find_map(|&id| subtree_point(doc, id, end))
    }
}

struct OffsetMapper<'a> {
    doc: &'a BaseDocument,
    root: NodeId,
    text: &'a str,
    cursor: usize,
    collapsed_space: bool,
    point: Point,
    result: Option<usize>,
}

impl OffsetMapper<'_> {
    fn visit(&mut self, id: NodeId) {
        let Some(node) = self.doc.get_node(id) else {
            return;
        };
        if node
            .primary_styles()
            .is_some_and(|style| style.clone_display() == style::values::computed::Display::None)
        {
            return;
        }
        if id != self.root
            && node
                .element_data()
                .is_some_and(|element| element.inline_layout_data.is_some())
        {
            return;
        }
        if let NodeData::Text(data) = &node.data {
            let style = node
                .parent
                .and_then(|id| self.doc.get_node(id))
                .and_then(Node::primary_styles);
            let transform = style
                .as_ref()
                .map_or(TextTransform::NONE, |style| style.clone_text_transform());
            let whitespace = style
                .as_ref()
                .map_or(WhiteSpaceCollapse::Collapse, |style| {
                    style.clone_white_space_collapse()
                });
            let mut units = 0;
            for c in data.content.chars() {
                if id == self.point.0 && units >= self.point.1 {
                    self.result = Some(self.cursor);
                    return;
                }
                self.advance(c, transform, whitespace);
                units += c.len_utf16();
            }
            if id == self.point.0 {
                self.result = Some(self.cursor);
            }
            return;
        }
        if node
            .element_data()
            .is_some_and(|element| element.name.local == blitz_dom::local_name!("br"))
        {
            self.advance('\n', TextTransform::NONE, WhiteSpaceCollapse::Preserve);
            return;
        }
        if let Some(before) = node.before() {
            self.visit(before);
        }
        for (index, &child) in node.children.iter().enumerate() {
            if id == self.point.0 && index == self.point.1 {
                self.result = Some(self.cursor);
                return;
            }
            self.visit(child);
            if self.result.is_some() {
                return;
            }
        }
        if id == self.point.0 {
            self.result = Some(self.cursor);
            return;
        }
        if let Some(after) = node.after() {
            self.visit(after);
        }
    }

    fn advance(&mut self, c: char, transform: TextTransform, whitespace: WhiteSpaceCollapse) {
        let remaining = &self.text[self.cursor..];
        let collapsible = match whitespace {
            WhiteSpaceCollapse::Collapse => c.is_ascii_whitespace(),
            WhiteSpaceCollapse::PreserveBreaks => matches!(c, ' ' | '\t'),
            _ => false,
        };
        if collapsible {
            if !self.collapsed_space {
                if remaining.starts_with(' ') {
                    self.cursor += 1;
                } else if transform.contains(TextTransform::FULL_WIDTH)
                    && remaining.starts_with('\u{3000}')
                {
                    self.cursor += '\u{3000}'.len_utf8();
                }
            }
            self.collapsed_space = true;
            return;
        }
        self.collapsed_space = false;
        let mut bytes = [0; 4];
        let original = c.encode_utf8(&mut bytes);
        if remaining.starts_with(&*original) {
            self.cursor += original.len();
            return;
        }
        // Consume case expansions only when they occur in the actual layout text.
        let map_width = |c| {
            let c = if transform.contains(TextTransform::FULL_WIDTH) {
                blitz_dom::full_width(c)
            } else {
                c
            };
            if transform.contains(TextTransform::FULL_SIZE_KANA) {
                blitz_dom::full_size_kana(c)
            } else {
                c
            }
        };
        let mapped = if transform.contains(TextTransform::UPPERCASE)
            || transform.contains(TextTransform::CAPITALIZE)
        {
            matching_prefix(remaining, c.to_uppercase().map(map_width))
        } else if transform.contains(TextTransform::LOWERCASE) {
            matching_prefix(remaining, c.to_lowercase().map(map_width))
        } else {
            None
        };
        if let Some(bytes) = mapped {
            self.cursor += bytes;
        } else if !transform.is_empty()
            && GeneralCategoryGroup::Mark
                .contains(CodePointMapData::<GeneralCategory>::new().get(c))
        {
            // Tailored casing can remove combining marks (e.g. Greek uppercase acute).
        } else if let Some(rendered) = remaining.chars().next() {
            // math-auto, full-width, full-size-kana and one-to-one tailored case mappings preserve
            // character count.
            self.cursor += rendered.len_utf8();
        }
    }
}

fn matching_prefix(text: &str, mapped: impl Iterator<Item = char>) -> Option<usize> {
    let mut text = text.chars();
    let mut bytes = 0;
    for c in mapped {
        if text.next() != Some(c) {
            return None;
        }
        bytes += c.len_utf8();
    }
    Some(bytes)
}
