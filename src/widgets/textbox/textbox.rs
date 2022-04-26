use dioxus::prelude::*;

use super::cursor::Cursor;

#[derive(Props, PartialEq)]
pub struct TextProps {
    initial_text: String,
}
#[allow(non_snake_case)]
pub fn Input(cx: Scope<TextProps>) -> Element {
    let text_ref = use_ref(&cx, || cx.props.initial_text.clone());
    let cursor = use_ref(&cx, || Cursor::default());

    let text = text_ref.read().clone();
    let start_highlight = cursor.read().first().idx(&text);
    let end_highlight = cursor.read().last().idx(&text);
    let (text_before_first_cursor, text_after_first_cursor) = text.split_at(start_highlight);
    let (text_highlighted, text_after_second_cursor) =
        text_after_first_cursor.split_at(end_highlight - start_highlight);
    println!("{text_before_first_cursor}|{text_highlighted}|{text_after_second_cursor}");

    cx.render({
        rsx! {
            div{
                border_style: "solid",
                border_width: "3px",
                border_radius: "3px",
                align_items: "left",

                // prevent tabing out of the textbox
                prevent_default: "onkeydown",
                onkeydown: |k| {
                    {
                        let mut text = text_ref.write();
                        cursor.write().handle_input(&*k, &mut text);
                    }
                },

                "{text_before_first_cursor}"
                span{
                    margin: "0px",
                    padding: "0px",
                    background_color: "rgba(100, 100, 100, 50%)",
                    height: "18px",

                    "|{text_highlighted}|"
                }
                "{text_after_second_cursor}"
            }
        }
    })
}
