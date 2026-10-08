use blitz_dom::text::EditableText as _;
//! Form-control values, text-input selection, and focus.

use super::attributes::{attr_getter, attr_setter, clear_attr, read_attr, write_attr};
use crate::dom::js_str;
use crate::dom::{dom_ctx, this_node_id};
use blitz_dom::NodeId;
use boa_engine::{Context, JsResult, JsValue};

pub(super) fn get_placeholder(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("placeholder", this, context)
}
pub(super) fn set_placeholder(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    attr_setter("placeholder", this, args, context)
}

pub(super) fn get_type(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("type", this, context)
}
pub(super) fn set_type(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    attr_setter("type", this, args, context)
}

pub(super) fn get_autofocus(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    Ok(JsValue::from(
        read_attr(&ctx, node_id, "autofocus").is_some(),
    ))
}
pub(super) fn set_autofocus(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let value = args.first().map(JsValue::to_boolean).unwrap_or(false);
    if value {
        write_attr(&ctx, node_id, "autofocus", "true");
    } else {
        clear_attr(&ctx, node_id, "autofocus");
    }
    Ok(JsValue::undefined())
}

pub(super) fn get_value(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let value = {
        let doc = ctx.doc.borrow();
        doc.get_node(node_id)
            .and_then(|node| node.element_data())
            .map(|element| match element.text_input_data() {
                Some(input_data) => input_data.editor.raw_text().to_string(),
                None => element
                    .attrs()
                    .iter()
                    .find(|attr| &*attr.name.local == "value")
                    .map(|attr| attr.value.clone())
                    .unwrap_or_default(),
            })
            .unwrap_or_default()
    };
    Ok(js_str(&value))
}

pub(super) fn set_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    attr_setter("value", this, args, context)
}

pub(super) fn get_checked(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let checked = {
        let doc = ctx.doc.borrow();
        doc.get_node(node_id)
            .and_then(|node| node.element_data())
            .map(|element| {
                element
                    .checkbox_input_checked()
                    .unwrap_or_else(|| element.attr(blitz_dom::local_name!("checked")).is_some())
            })
            .unwrap_or(false)
    };
    Ok(JsValue::from(checked))
}

pub(super) fn set_checked(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let checked = args.first().map(JsValue::to_boolean).unwrap_or(false);
    // blitz-dom's checked handling parses the value as a boolean
    write_attr(
        &ctx,
        node_id,
        "checked",
        if checked { "true" } else { "false" },
    );
    Ok(JsValue::undefined())
}

// === Text input selection (`selectionStart`/`selectionEnd`) ===
//
// Backed by the text input's real editor selection. Offsets are in UTF-16 code
// units, per spec. Returns null for elements without a text editor. React
// snapshots these around controlled-input re-renders and restores the caret
// via `setSelectionRange()`.

/// Convert a byte offset in `text` to a UTF-16 code-unit offset
fn byte_to_utf16(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

/// Convert a UTF-16 code-unit offset to a byte offset in `text` (clamped)
fn utf16_to_byte(text: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (byte_offset, ch) in text.char_indices() {
        if units >= utf16_offset {
            return byte_offset;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// The current selection of the node's text editor as UTF-16 offsets
fn selection_utf16_range(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> Option<(usize, usize)> {
    let input = doc.get_node(node_id)?.element_data()?.text_input_data()?;
    let text = input.editor.raw_text();
    let range = input.editor.selection();
    Some((
        byte_to_utf16(text, range.start),
        byte_to_utf16(text, range.end),
    ))
}

/// Set the node's text editor selection from UTF-16 offsets
fn set_selection_utf16_range(
    doc: &mut blitz_dom::BaseDocument,
    node_id: NodeId,
    start: usize,
    end: usize,
) {
    doc.with_text_input(node_id, |mut driver| {
        let text = driver.raw_text().to_string();
        let start_byte = utf16_to_byte(&text, start);
        let end_byte = utf16_to_byte(&text, start.max(end));
        driver.select_byte_range(start_byte, end_byte);
    });
}

pub(super) fn get_selection_start(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    // The text editor is created during layout construction
    doc.resolve(0.0);
    Ok(match selection_utf16_range(&doc, node_id) {
        Some((start, _)) => JsValue::from(start as u32),
        None => JsValue::null(),
    })
}

pub(super) fn get_selection_end(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    // The text editor is created during layout construction
    doc.resolve(0.0);
    Ok(match selection_utf16_range(&doc, node_id) {
        Some((_, end)) => JsValue::from(end as u32),
        None => JsValue::null(),
    })
}

pub(super) fn set_selection_start(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let start = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    if let Some((_, end)) = selection_utf16_range(&doc, node_id) {
        set_selection_utf16_range(&mut doc, node_id, start, end.max(start));
    }
    Ok(JsValue::undefined())
}

pub(super) fn set_selection_end(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let end = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    if let Some((start, _)) = selection_utf16_range(&doc, node_id) {
        set_selection_utf16_range(&mut doc, node_id, start.min(end), end);
    }
    Ok(JsValue::undefined())
}

pub(super) fn set_selection_range(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let start = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let end = args
        .get(1)
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    set_selection_utf16_range(&mut doc, node_id, start, end);
    Ok(JsValue::undefined())
}

// === Focus ===

pub(super) fn focus(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    ctx.doc.borrow_mut().set_focus_to(node_id);
    Ok(JsValue::undefined())
}

pub(super) fn blur(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    ctx.doc.borrow_mut().clear_focus();
    Ok(JsValue::undefined())
}
