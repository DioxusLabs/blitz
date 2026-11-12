//! Selection API bindings and DOM-to-rendered text offset mapping.

use std::cell::Cell;

use blitz_dom::text::InlineText as _;
use blitz_dom::{BaseDocument, Node, NodeId, node::NodeData};
use boa_engine::{
    Context, Finalize, JsData, JsNativeError, JsResult, JsValue, Trace, object::JsObject,
};

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
