//! Drop files, folders or text from another application to preview them

use std::io::Read;
use std::path::Path;

use dioxus_native::prelude::*;
use url::Url;

/// How much of a text file is read for its preview
const TEXT_PREVIEW_BYTES: u64 = 4096;

fn main() {
    dioxus_native::launch(app);
}

#[derive(Clone, PartialEq)]
struct DroppedItem {
    kind: &'static str,
    name: String,
    /// The full path of a file or folder
    path: Option<String>,
    /// The size of a file, or the number of entries in a folder
    detail: Option<String>,
    preview: Preview,
}

#[derive(Clone, PartialEq)]
enum Preview {
    /// The URL of an image
    Image(String),
    /// The URL of an HTML document
    HtmlUrl(String),
    /// HTML markup
    Html(String),
    Text(String),
    None,
}

impl DroppedItem {
    fn from_path(path: &Path) -> Self {
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let url = || Url::from_file_path(path).ok().map(String::from);

        let detail = if path.is_dir() {
            std::fs::read_dir(path)
                .ok()
                .map(|entries| match entries.count() {
                    1 => "1 item".to_string(),
                    count => format!("{count} items"),
                })
        } else {
            std::fs::metadata(path)
                .ok()
                .map(|metadata| format_size(metadata.len()))
        };

        let (kind, preview) = if path.is_dir() {
            ("Folder", Preview::None)
        } else if let "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" = extension.as_str()
            && let Some(url) = url()
        {
            ("Image", Preview::Image(url))
        } else if let "html" | "htm" = extension.as_str()
            && let Some(url) = url()
        {
            ("HTML", Preview::HtmlUrl(url))
        } else if let Some(text) = read_text_preview(path) {
            ("Text", Preview::Text(text))
        } else {
            ("File", Preview::None)
        };

        Self {
            kind,
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: Some(path.display().to_string()),
            detail,
            preview,
        }
    }

    fn from_data(kind: &'static str, name: &str, preview: Preview) -> Self {
        Self {
            kind,
            name: name.to_string(),
            path: None,
            detail: None,
            preview,
        }
    }
}

fn format_size(bytes: u64) -> String {
    match bytes {
        0..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{:.1} kB", bytes as f64 / 1e3),
        _ => format!("{:.1} MB", bytes as f64 / 1e6),
    }
}

/// The start of a file, if the file is text
fn read_text_preview(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).ok()?;
    file.take(TEXT_PREVIEW_BYTES).read_to_end(&mut bytes).ok()?;

    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        // The preview may end part way through a character
        Err(err) if err.error_len().is_none() => {
            std::str::from_utf8(&bytes[..err.valid_up_to()]).ok()?
        }
        Err(_) => return None,
    };
    (!text.is_empty() && !text.contains('\0')).then(|| text.to_string())
}

fn app() -> Element {
    // `dragenter` and `dragleave` bubble up from the children of the drop zone, so count them
    // rather than tracking a single flag
    let mut drag_depth = use_signal(|| 0u32);
    let mut items = use_signal(Vec::<DroppedItem>::new);

    rsx! {
        style { {CSS} }
        main {
            h1 { "File drop" }
            div {
                class: if drag_depth() > 0 { "drop-zone active" } else { "drop-zone" },
                ondragenter: move |_| drag_depth += 1,
                ondragover: move |evt| evt.prevent_default(),
                ondragleave: move |_| drag_depth.set(drag_depth().saturating_sub(1)),
                ondrop: move |evt| {
                    evt.prevent_default();
                    drag_depth.set(0);

                    let data = evt.data_transfer();
                    let mut dropped: Vec<DroppedItem> = data
                        .files()
                        .iter()
                        .map(|file| DroppedItem::from_path(&file.path()))
                        .collect();
                    // Dragged text usually comes in several formats: preview the richest one
                    if let Some(html) = data.get_data("text/html") {
                        dropped.push(DroppedItem::from_data("HTML", "text/html", Preview::Html(html)));
                    } else if let Some(text) = data.get_data("text/plain") {
                        dropped.push(DroppedItem::from_data("Text", "text/plain", Preview::Text(text)));
                    } else if let Some(links) = data.get_data("text/uri-list") {
                        dropped.push(DroppedItem::from_data("Link", "text/uri-list", Preview::Text(links)));
                    }
                    // A new drop replaces the last one
                    items.set(dropped);
                },
                svg {
                    class: "icon",
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: if drag_depth() > 0 { "#1d4ed8" } else { "#6b7280" },
                    stroke_width: "1.5",
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    path { d: "M12 3v12m0 0l-5-5m5 5l5-5M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" }
                }
                div { class: "title",
                    if drag_depth() > 0 { "Release to drop" } else { "Drop files, folders or text here" }
                }
                div { class: "hint", "Images, HTML and text are previewed" }
            }
            div { class: "items",
                for item in items.iter() {
                    Item { item: item.clone() }
                }
            }
        }
    }
}

#[component]
fn Item(item: DroppedItem) -> Element {
    rsx! {
        div { class: "item",
            div { class: "header",
                span { class: "kind", "{item.kind}" }
                span { class: "name", "{item.name}" }
                if let Some(detail) = &item.detail {
                    span { class: "detail", "{detail}" }
                }
            }
            if let Some(path) = &item.path {
                div { class: "path", "{path}" }
            }
            match &item.preview {
                Preview::Image(url) => rsx! {
                    div { class: "preview image",
                        img { src: "{url}" }
                    }
                },
                Preview::HtmlUrl(url) => rsx! {
                    iframe { class: "preview", src: "{url}" }
                },
                Preview::Html(html) => rsx! {
                    iframe { class: "preview", srcdoc: "{html}" }
                },
                Preview::Text(text) => rsx! {
                    pre { class: "preview", "{text}" }
                },
                Preview::None => rsx! {},
            }
        }
    }
}

const CSS: &str = r#"
    html { background: #f3f4f6; }
    body { margin: 0; font-family: system-ui, sans-serif; color: #1f2937; }
    main { max-width: 880px; margin: 0 auto; padding: 32px 24px; }
    h1 { margin: 0 0 16px; font-size: 22px; }

    .drop-zone {
        padding: 36px 24px;
        border: 2px dashed #9ca3af;
        border-radius: 12px;
        background: white;
        color: #6b7280;
        text-align: center;
    }
    .drop-zone.active {
        border: 2px solid #2563eb;
        background: #dbeafe;
        color: #1d4ed8;
    }
    .drop-zone .icon { width: 40px; height: 40px; }
    .drop-zone .title { margin-top: 8px; font-size: 18px; font-weight: 600; }
    .drop-zone .hint { margin-top: 4px; font-size: 13px; }

    .items {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
        gap: 16px;
        margin-top: 24px;
    }
    .item {
        min-width: 0;
        padding: 12px;
        border: 1px solid #e5e7eb;
        border-radius: 10px;
        background: white;
    }
    .item .header { display: flex; align-items: center; gap: 8px; }
    .item .kind {
        flex: none;
        padding: 2px 8px;
        border-radius: 999px;
        background: #e0e7ff;
        color: #3730a3;
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
    }
    .item .name { min-width: 0; font-weight: 600; overflow-wrap: anywhere; }
    .item .detail { flex: none; margin-left: auto; color: #6b7280; font-size: 12px; }
    .item .path {
        margin-top: 6px;
        color: #6b7280;
        font-family: monospace;
        font-size: 12px;
        overflow-wrap: anywhere;
    }
    .item .preview {
        display: block;
        box-sizing: border-box;
        width: 100%;
        height: 200px;
        margin: 10px 0 0;
        border: 1px solid #e5e7eb;
        border-radius: 6px;
        background: #f9fafb;
        overflow: hidden;
    }
    .item .preview.image { display: flex; align-items: center; justify-content: center; }
    .item .preview.image img { max-width: 100%; max-height: 100%; }
    .item pre.preview {
        padding: 8px;
        font-size: 12px;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }
"#;
