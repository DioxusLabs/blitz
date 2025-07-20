use markup5ever::{LocalName, local_name};

use crate::{
    BaseDocument, ElementData,
    traversal::{AncestorTraverser, TreeTraverser},
};
use blitz_traits::navigation::NavigationOptions;
use core::str::FromStr;
use std::{borrow::Cow, fmt::Display, path::Path};

impl BaseDocument {
    /// Resets the form owner for a given node by either using an explicit form attribute
    /// or finding the nearest ancestor form element
    ///
    /// # Arguments
    /// * `node_id` - The ID of the node whose form owner needs to be reset
    ///
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#reset-the-form-owner>
    pub fn reset_form_owner(&mut self, node_id: usize) {
        let node = &self.nodes[node_id];
        let Some(element) = node.element_data() else {
            return;
        };

        // First try explicit form attribute
        let final_owner_id = element
            .attr(local_name!("form"))
            .and_then(|owner| self.nodes_to_id.get(owner))
            .copied()
            .filter(|owner_id| {
                self.get_node(*owner_id)
                    .is_some_and(|node| node.data.is_element_with_tag_name(&local_name!("form")))
            })
            .or_else(|| {
                AncestorTraverser::new(self, node_id).find(|ancestor_id| {
                    self.nodes[*ancestor_id]
                        .data
                        .is_element_with_tag_name(&local_name!("form"))
                })
            });

        if let Some(final_owner_id) = final_owner_id {
            self.controls_to_form.insert(node_id, final_owner_id);
        }
    }

    /// Submits a form with the given form node ID and submitter node ID
    ///
    /// # Arguments
    /// * `node_id` - The ID of the form node to submit
    /// * `submitter_id` - The ID of the node that triggered the submission
    ///
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>
    pub fn submit_form(&self, node_id: usize, submitter_id: usize) {
        let node = &self.nodes[node_id];
        let Some(element) = node.element_data() else {
            return;
        };

        let mut entry = construct_entry_list(self, node_id, submitter_id);

        let method = get_form_attr(
            self,
            element,
            local_name!("method"),
            submitter_id,
            local_name!("formmethod"),
        )
        .and_then(|method| method.parse::<FormMethod>().ok())
        .unwrap_or(FormMethod::Get);

        let action = get_form_attr(
            self,
            element,
            local_name!("action"),
            submitter_id,
            local_name!("formaction"),
        )
        .unwrap_or_default();

        let mut parsed_action = self.resolve_url(action);

        let scheme = parsed_action.scheme();

        let mut enctype = get_form_attr(
            self,
            element,
            local_name!("enctype"),
            submitter_id,
            local_name!("formenctype"),
        )
        .and_then(|enctype| enctype.parse::<RequestContentType>().ok())
        .unwrap_or(RequestContentType::FormUrlEncoded);

        let mut post_resource = None;

        match (scheme, method) {
            ("http" | "https" | "data", FormMethod::Get) => {
                let pairs = entry.convert_to_list_of_name_value_pairs();

                let mut query = String::new();
                url::form_urlencoded::Serializer::new(&mut query).extend_pairs(pairs);

                parsed_action.set_query(Some(&query));
            }

            ("http" | "https", FormMethod::Post) => match enctype {
                RequestContentType::FormUrlEncoded => {
                    let pairs = entry.convert_to_list_of_name_value_pairs();
                    let mut body = String::new();
                    url::form_urlencoded::Serializer::new(&mut body).extend_pairs(pairs);
                    post_resource = Some(body.into());
                }
                RequestContentType::MultipartFormData(_) => {
                    let (encoded, boundary) = entry.encode_multipart_form_data();
                    post_resource = Some(encoded.into());
                    enctype = RequestContentType::MultipartFormData(boundary);
                }
                RequestContentType::TextPlain => {
                    let pairs = entry.convert_to_list_of_name_value_pairs();
                    let body = encode_text_plain(&pairs).into();
                    post_resource = Some(body);
                }
            },
            ("mailto", FormMethod::Get) => {
                let pairs = entry.convert_to_list_of_name_value_pairs();

                parsed_action.query_pairs_mut().extend_pairs(pairs);
            }
            ("mailto", FormMethod::Post) => {
                let pairs = entry.convert_to_list_of_name_value_pairs();
                let body = match enctype {
                    RequestContentType::TextPlain => {
                        let body = encode_text_plain(&pairs);

                        /// https://url.spec.whatwg.org/#default-encode-set
                        const DEFAULT_ENCODE_SET: percent_encoding::AsciiSet =
                            percent_encoding::CONTROLS
                                // Query Set
                                .add(b' ')
                                .add(b'"')
                                .add(b'#')
                                .add(b'<')
                                .add(b'>')
                                // Path Set
                                .add(b'?')
                                .add(b'`')
                                .add(b'{')
                                .add(b'}');

                        // Set body to the result of running UTF-8 percent-encode on body using the default encode set. [URL]
                        percent_encoding::utf8_percent_encode(&body, &DEFAULT_ENCODE_SET)
                            .to_string()
                    }
                    _ => {
                        let mut body = String::new();
                        url::form_urlencoded::Serializer::new(&mut body).extend_pairs(pairs);
                        body
                    }
                };
                let mut query = if let Some(query) = parsed_action.query() {
                    let mut query = query.to_string();
                    query.push('&');
                    query
                } else {
                    String::new()
                };
                query.push_str("body=");
                query.push_str(&body);
                parsed_action.set_query(Some(&query));
            }
            _ => {
                #[cfg(feature = "tracing")]
                tracing::warn!(
                    "Scheme {} with method {:?} is not implemented",
                    scheme,
                    method
                );
                return;
            }
        }

        let navigation_options =
            NavigationOptions::new(parsed_action, enctype.to_string(), self.id())
                .set_document_resource(post_resource);

        self.navigation_provider.navigate_to(navigation_options)
    }
}

/// Constructs a list of form entries from form controls
///
/// # Arguments
/// * `doc` - Reference to the base document
/// * `form_id` - ID of the form element
/// * `submitter_id` - ID of the element that triggered form submission
///
/// # Returns
/// Returns an EntryList containing all valid form control entries
///
/// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#constructing-the-form-data-set
fn construct_entry_list(doc: &BaseDocument, form_id: usize, submitter_id: usize) -> EntryList {
    let mut entry_list = EntryList::new();

    let mut create_entry = |name: &str, value: EntryValue| {
        entry_list.0.push(Entry::new(name, value));
    };

    fn datalist_ancestor(doc: &BaseDocument, node_id: usize) -> bool {
        AncestorTraverser::new(doc, node_id).any(|node_id| {
            doc.nodes[node_id]
                .data
                .is_element_with_tag_name(&local_name!("datalist"))
        })
    }

    for control_id in TreeTraverser::new(doc) {
        let Some(node) = doc.get_node(control_id) else {
            continue;
        };
        let Some(element) = node.element_data() else {
            continue;
        };

        // Check if the form owner is same as form_id
        if doc
            .controls_to_form
            .get(&control_id)
            .map(|owner_id| *owner_id != form_id)
            .unwrap_or(true)
        {
            continue;
        }

        let element_type = element.attr(local_name!("type"));

        //  If any of the following are true:
        //   field has a datalist element ancestor;
        //   field is disabled;
        //   field is a button but it is not submitter;
        //   field is an input element whose type attribute is in the Checkbox state and whose checkedness is false; or
        //   field is an input element whose type attribute is in the Radio Button state and whose checkedness is false,
        //  then continue.
        if datalist_ancestor(doc, node.id)
            || element.attr(local_name!("disabled")).is_some()
            || (element.name.local == local_name!("button") && node.id != submitter_id)
            || element.name.local == local_name!("input")
                && ((matches!(element_type, Some("checkbox" | "radio"))
                    && !element.checkbox_input_checked().unwrap_or(false))
                    || matches!(element_type, Some("submit" | "button")))
        {
            continue;
        }

        // If the field element is an input element whose type attribute is in the Image Button state, then:
        if element_type == Some("image") {
            // If the field element is not submitter, then continue.
            if node.id != submitter_id {
                continue;
            }
            // TODO: If the field element has a name attribute specified and its value is not the empty string, let name be that value followed by U+002E (.). Otherwise, let name be the empty string.
            //   Let namex be the concatenation of name and U+0078 (x).
            //   Let namey be the concatenation of name and U+0079 (y).
            //   Let (x, y) be the selected coordinate.
            //   Create an entry with namex and x, and append it to entry list.
            //   Create an entry with namey and y, and append it to entry list.
            //   Continue.
            continue;
        }

        // TODO: If the field is a form-associated custom element,
        //  then perform the entry construction algorithm given field and entry list,
        //  then continue.

        //     If either the field element does not have a name attribute specified, or its name attribute's value is the empty string, then continue.
        //     Let name be the value of the field element's name attribute.
        let Some(name) = element
            .attr(local_name!("name"))
            .filter(|str| !str.is_empty())
        else {
            continue;
        };

        // TODO: If the field element is a select element,
        //  then for each option element in the select element's
        //  list of options whose selectedness is true and that is not disabled,
        //  create an entry with name and the value of the option element,
        //  and append it to entry list.

        // Otherwise, if the field element is an input element whose type attribute is in the Checkbox state or the Radio Button state, then:
        if element.name.local == local_name!("input")
            && matches!(element_type, Some("checkbox" | "radio"))
        {
            // If the field element has a value attribute specified, then let value be the value of that attribute; otherwise, let value be the string "on".
            let value = element.attr(local_name!("value")).unwrap_or("on");
            //         Create an entry with name and value, and append it to entry list.
            create_entry(name, value.into());
        }
        // Otherwise, if the field element is an input element whose type attribute is in the File Upload state, then:
        else if element.name.local == local_name!("input") && matches!(element_type, Some("file"))
        {
            //        If there are no selected files, then create an entry with name and a new File object with an empty name, application/octet-stream as type, and an empty body, and append it to entry list.

            let Some(files) = element.file_data() else {
                create_entry(name, File::empty().into());
                continue;
            };
            if files.is_empty() {
                create_entry(name, File::empty().into());
            }
            //        Otherwise, for each file in selected files, create an entry with name and a File object representing the file, and append it to entry list.
            else {
                for file in files.iter() {
                    create_entry(name, File::from_path(file).into());
                }
            }
        }
        //Otherwise, if the field element is an input element whose type attribute is in the Hidden state and name is an ASCII case-insensitive match for "_charset_":
        else if element.name.local == local_name!("input")
            && element_type == Some("hidden")
            && name.eq_ignore_ascii_case("_charset_")
        {
            // Let charset be the name of encoding.
            let charset = "UTF-8"; // TODO: Support multiple encodings.
            // Create an entry with name and charset, and append it to entry list.
            create_entry(name, charset.into());
        }
        // Otherwise, create an entry with name and the value of the field element, and append it to entry list.
        else if let Some(text) = element.text_input_data() {
            create_entry(name, text.editor.text().to_string().into());
        } else if let Some(value) = element.attr(local_name!("value")) {
            create_entry(name, value.into());
        }
    }
    entry_list
}

/// Normalizes line endings in a string according to HTML spec
///
/// Converts single CR or LF to CRLF pairs according to HTML form submission requirements
///
/// # Arguments
/// * `input` - The string whose line endings need to be normalized
///
/// # Returns
/// A new string with normalized CRLF line endings
fn normalize_line_endings(input: &str) -> String {
    // Replace every occurrence of U+000D (CR) not followed by U+000A (LF),
    // and every occurrence of U+000A (LF) not preceded by U+000D (CR),
    // in value, by a string consisting of U+000D (CR) and U+000A (LF).

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(current) = chars.next() {
        match (current, chars.peek()) {
            ('\r', Some('\n')) => {
                result.push_str("\r\n");
                chars.next();
            }
            ('\r' | '\n', _) => {
                result.push_str("\r\n");
            }
            _ => result.push(current),
        }
    }

    result
}

fn get_form_attr<'a>(
    doc: &'a BaseDocument,
    form: &'a ElementData,
    form_local: impl PartialEq<LocalName>,
    submitter_id: usize,
    submitter_local: impl PartialEq<LocalName>,
) -> Option<&'a str> {
    get_submitter_attr(doc, submitter_id, submitter_local).or_else(|| form.attr(form_local))
}

fn get_submitter_attr(
    doc: &BaseDocument,
    submitter_id: usize,
    local_name: impl PartialEq<LocalName>,
) -> Option<&str> {
    doc.get_node(submitter_id)
        .and_then(|node| node.element_data())
        .and_then(|element_data| {
            if element_data.name.local == local_name!("button")
                && element_data.attr(local_name!("type")) == Some("submit")
            {
                element_data.attr(local_name)
            } else {
                None
            }
        })
}
/// Encodes form data as text/plain according to HTML spec
///
/// # Arguments
/// * `input` - Slice of name-value pairs to encode
///
/// # Returns
/// A string with the encoded form data
///
/// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#text/plain-encoding-algorithm
fn encode_text_plain(input: &[(String, String)]) -> String {
    let mut out = String::new();
    for (name, value) in input {
        out.push_str(name);
        out.push('=');
        out.push_str(value);
        out.push_str("\r\n");
    }
    out
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum FormMethod {
    Get,
    Post,
    Dialog,
}
impl FromStr for FormMethod {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "get" => FormMethod::Get,
            "post" => FormMethod::Post,
            "dialog" => FormMethod::Dialog,
            _ => return Err(()),
        })
    }
}

/// Supported content types for HTTP requests
#[derive(Debug, Clone)]
pub enum RequestContentType {
    /// application/x-www-form-urlencoded
    FormUrlEncoded,
    /// multipart/form-data
    MultipartFormData(String),
    /// text/plain
    TextPlain,
}

impl FromStr for RequestContentType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "application/x-www-form-urlencoded" => RequestContentType::FormUrlEncoded,
            "multipart/form-data" => RequestContentType::MultipartFormData(String::new()),
            "text/plain" => RequestContentType::TextPlain,
            _ => return Err(()),
        })
    }
}

impl Display for RequestContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestContentType::FormUrlEncoded => write!(f, "application/x-www-form-urlencoded"),
            RequestContentType::MultipartFormData(boundary) if boundary.is_empty() => {
                write!(f, "multipart/form-data")
            }
            RequestContentType::MultipartFormData(boundary) => {
                write!(f, "multipart/form-data; boundary={boundary}")
            }
            RequestContentType::TextPlain => write!(f, "text/plain"),
        }
    }
}

/// A list of form entries used for form submission
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EntryList(Vec<Entry>);
impl EntryList {
    /// Creates a new empty EntryList
    pub fn new() -> Self {
        EntryList(Vec::new())
    }

    /// Converts the entry list to a vector of name-value pairs with normalized line endings
    /// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#converting-an-entry-list-to-a-list-of-name-value-pairs
    pub fn convert_to_list_of_name_value_pairs(&self) -> Vec<(String, String)> {
        self.0
            .iter()
            .map(|entry| {
                let name = normalize_line_endings(&entry.name);

                let value = match entry.value {
                    EntryValue::String(ref value) => value,
                    EntryValue::File(ref file) => &file.name,
                };

                let value = normalize_line_endings(value);
                (name, value)
            })
            .collect()
    }

    /// Encodes the entry list as multipart/form-data
    ///
    /// The multipart/form-data encoding algorithm, given an entry list entry list and an encoding encoding, is as follows:
    /// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#multipart-form-data
    ///
    /// NOTE: We don't have the encoding parameter as we only support UTF-8 encoding.
    pub fn encode_multipart_form_data(&mut self) -> (Vec<u8>, String) {
        let boundary = generate_boundary();
        let mut output = Vec::new();

        // 1. For each entry of entry list:
        self.0
            .drain(..)
            .map(|Entry { name, value }| {
                (
                    // 1. Replace every occurrence of U+000D (CR) not followed by U+000A (LF), and every occurrence of U+000A (LF) not preceded by U+000D (CR), in entry's name, by a string consisting of a U+000D (CR) and U+000A (LF).
                    normalize_line_endings(&name),
                    // 2. If entry's value is not a File object, then replace every occurrence of U+000D (CR) not followed by U+000A (LF), and every occurrence of U+000A (LF) not preceded by U+000D (CR), in entry's value, by a string consisting of a U+000D (CR) and U+000A (LF).
                    if let EntryValue::String(string) = value {
                        EntryValue::String(normalize_line_endings(&string))
                    } else {
                        value
                    },
                )
            })
            // 2. Return the byte sequence resulting from encoding the entry list using the rules described by RFC 7578, Returning Values from Forms: multipart/form-data, given the following conditions: [https://www.rfc-editor.org/rfc/rfc7578]
            .for_each(|(name, value)| create_part(&mut output, &name, &value, &boundary));

        last_boundary(&mut output, &boundary);
        (output, boundary)
    }
}

/// A single form entry consisting of a name and value
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    name: String,
    value: EntryValue,
}

impl Entry {
    fn new(name: &str, value: EntryValue) -> Self {
        Self {
            name: name.to_string(),
            value,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum EntryValue {
    String(String),
    File(File),
}

impl From<String> for EntryValue {
    fn from(value: String) -> Self {
        EntryValue::String(value)
    }
}
impl From<&str> for EntryValue {
    fn from(value: &str) -> Self {
        EntryValue::String(value.to_string())
    }
}
impl From<File> for EntryValue {
    fn from(value: File) -> Self {
        EntryValue::File(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct File {
    name: String,
    content_type: String,
    data: Vec<u8>,
}
impl File {
    ///FIXME: Follow the spec https://w3c.github.io/FileAPI/#file-constructor
    pub fn new(name: &str, ty: &str, data: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            content_type: ty.to_string(),
            data,
        }
    }
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            content_type: "application/octet-stream".to_string(),
            data: Vec::new(),
        }
    }
    pub fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default();
        let file = std::fs::read(path).unwrap();
        // TODO: use proper content type
        Self::new(name, "application/octet-stream", file)
    }
}

/// Generates a random boundary string for multipart/form-data
fn generate_boundary() -> String {
    format!(
        "BlitzFormBoundary-{}",
        (0..10)
            .map(|_| fastrand::alphanumeric())
            .collect::<String>()
    )
}

/// Creates a part of a multipart/form-data body
fn create_part<W: std::io::Write>(w: &mut W, name: &str, value: &EntryValue, boundary: &str) {
    //TODO: Either do this by removing from CONTROLS or encode this without the percent encoding crate
    const MINIMAL_ENCODE_SET: percent_encoding::AsciiSet =
        unsafe { std::mem::transmute::<[u32; 4], percent_encoding::AsciiSet>([0u32; 4]) }
            .add(b'\n')
            .add(b'\r')
            .add(b'"');

    // --{boundary}\r\n
    w.write_all(b"--").unwrap();
    w.write_all(boundary.as_bytes()).unwrap();
    w.write_all(b"\r\n").unwrap();

    // Content-Disposition: form-data; name="{name}"
    w.write_all(b"Content-Disposition: form-data; name=\"")
        .unwrap();

    let encoded_name = Cow::from(percent_encoding::utf8_percent_encode(
        name,
        &MINIMAL_ENCODE_SET,
    ));
    w.write_all(encoded_name.as_bytes()).unwrap();
    w.write_all(b"\"").unwrap();

    match value {
        EntryValue::String(content) => {
            // \r\n\r\n (end headers, then blank line before content)
            w.write_all(b"\r\n\r\n").unwrap();

            // {content}
            w.write_all(content.as_bytes()).unwrap();
        }
        EntryValue::File(file) => {
            if !file.name.is_empty() {
                // ; filename="{file.name}"
                w.write_all(b"; filename=\"").unwrap();
                let encoded_filename = Cow::from(percent_encoding::utf8_percent_encode(
                    &file.name,
                    &MINIMAL_ENCODE_SET,
                ));
                w.write_all(encoded_filename.as_bytes()).unwrap();
                w.write_all(b"\"").unwrap();
            }

            // \r\n
            w.write_all(b"\r\n").unwrap();

            // Content-Type: {content_type}\r\n\r\n
            w.write_all(b"Content-Type: ").unwrap();
            w.write_all(file.content_type.as_bytes()).unwrap();
            w.write_all(b"\r\n\r\n").unwrap();

            // {file data}
            w.write_all(&file.data).unwrap();
        }
    };

    // \r\n (end of part)
    w.write_all(b"\r\n").unwrap();
}

/// Adds the end boundary to the multipart/form-data body
fn last_boundary<W: std::io::Write>(w: &mut W, boundary: &str) {
    // --{boundary}--
    w.write_all(b"--").unwrap();
    w.write_all(boundary.as_bytes()).unwrap();
    w.write_all(b"--\r\n").unwrap();
}
