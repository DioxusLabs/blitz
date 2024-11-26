mod markdown;
use blitz_dom::net::Resource;
use blitz_dom::HtmlDocument;
use blitz_net::Provider;
use blitz_traits::net::SharedCallback;
use markdown::{markdown_to_html, BLITZ_MD_STYLES, GITHUB_MD_STYLES};
use reqwest::header::HeaderName;

use dioxus_native::{create_default_event_loop, WinitNetCallback};
use std::env::current_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::runtime::Handle;
use url::Url;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
fn main() {
    let raw_url = std::env::args().nth(1);

    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _guard = rt.enter();

    let event_loop = create_default_event_loop();
    let proxy = event_loop.create_proxy();

    let (base_url, contents, is_md) = rt.block_on(fetch(raw_url));

    // Process markdown if necessary
    let mut html = contents;
    let mut stylesheets = Vec::new();
    if is_md {
        html = markdown_to_html(html);
        stylesheets.push(String::from(GITHUB_MD_STYLES));
        stylesheets.push(String::from(BLITZ_MD_STYLES));
    }

    // println!("{html}");

    let net_callback = Arc::new(WinitNetCallback::new(proxy));
    let net_provider = Arc::new(Provider::new(
        Handle::current(),
        Arc::clone(&net_callback) as SharedCallback<Resource>,
    ));

    let document = HtmlDocument::from_html(&html, Some(base_url), stylesheets, net_provider, None);
    dioxus_native::launch_with_document(document, rt, event_loop);
}

async fn fetch(raw_url: Option<String>) -> (String, String, bool) {
    match raw_url {
        None => {
            let cwd = current_dir().ok();
            let mut maybe_dir = cwd.as_deref();
            while let Some(dir) = maybe_dir {
                let path = dir.join("README.md");
                if fs::exists(&path).unwrap() {
                    let base_url = format!("file://{}/", dir.display());
                    let contents = std::fs::read_to_string(&path).unwrap();
                    return (base_url, contents, true);
                }

                maybe_dir = dir.parent()
            }

            eprintln!("Could not find README.md file in the current directory");
            std::process::exit(1);
        }
        Some(raw_url) => {
            if let Ok(url) = Url::parse(&raw_url) {
                match url.scheme() {
                    "file" => {
                        let raw_file_content = std::fs::read(url.path()).unwrap();
                        let file_content = String::from_utf8(raw_file_content).unwrap();
                        let base_url = PathBuf::from(&raw_url)
                            .parent()
                            .unwrap()
                            .to_string_lossy()
                            .to_string();
                        let is_md = raw_url.ends_with(".md");
                        (base_url, file_content, is_md)
                    }
                    _ => {
                        let client = reqwest::Client::new();
                        let response = client
                            .get(url)
                            .header("User-Agent", USER_AGENT)
                            .send()
                            .await
                            .unwrap();
                        let content_type = response
                            .headers()
                            .get(HeaderName::from_static("content-type"));
                        let is_md = raw_url.ends_with(".md")
                            || content_type.is_some_and(|ct| {
                                ct.to_str().is_ok_and(|ct| ct.starts_with("text/markdown"))
                            });
                        let file_content = response.text().await.unwrap();

                        (raw_url, file_content, is_md)
                    }
                }
            } else if fs::exists(&raw_url).unwrap() {
                let base_path = std::path::absolute(Path::new(&raw_url)).unwrap();
                let base_path = base_path.parent().unwrap().to_string_lossy();
                let base_url = format!("file://{}/", base_path);
                let contents = std::fs::read_to_string(&raw_url).unwrap();
                let is_md = raw_url.ends_with(".md");

                (base_url, contents, is_md)
            } else {
                eprintln!("Cannot parse {} as url or find it as a file", raw_url);
                std::process::exit(1);
            }
        }
    }
}
