//! Render the readme.md using the gpu renderer

use std::{collections::HashMap, path::Path};

use comrak::{
    adapters::SyntaxHighlighterAdapter, markdown_to_html_with_plugins,
    plugins::syntect::SyntectAdapter, ExtensionOptionsBuilder, Options, Plugins,
    RenderOptionsBuilder,
};
use dioxus_native::Config;

fn main() {
    let (base_url, contents) = std::env::args()
        .skip(1)
        .next()
        .map(|path| {
            let base_path = std::path::absolute(Path::new(&path)).unwrap();
            let base_path = base_path.parent().unwrap().to_string_lossy();
            let base_url = format!("file://{}/", base_path);
            let contents = std::fs::read_to_string(path).unwrap();
            (base_url, contents)
        })
        .unwrap_or({
            let base_url = "https://raw.githubusercontent.com/DioxusLabs/blitz/main/".to_string();
            let contents = include_str!("../README.md").to_string();
            (base_url, contents)
        });

    let plugins = Plugins::default();
    // let syntax_highligher = CustomSyntectAdapter(SyntectAdapter::new(Some("InspiredGitHub")));
    // plugins.render.codefence_syntax_highlighter = Some(&syntax_highligher as _);

    let stylesheet = include_str!("./assets/github-markdown-light.css");
    let body_html = markdown_to_html_with_plugins(
        &contents,
        &Options {
            extension: ExtensionOptionsBuilder::default()
                .strikethrough(true)
                .tagfilter(false)
                .table(true)
                .autolink(true)
                .tasklist(true)
                .superscript(false)
                .header_ids(None)
                .footnotes(false)
                .description_lists(false)
                .front_matter_delimiter(None)
                .multiline_block_quotes(false)
                .build()
                .unwrap(),
            render: RenderOptionsBuilder::default()
                .unsafe_(true)
                .tasklist_classes(true)
                .build()
                .unwrap(),
            ..Default::default()
        },
        &plugins,
    );

    let html = format!(
        r#"
        <!DOCTYPE html>
        <html>
        <body>
        <div class="markdown-body">{}</div>
        </body>
        </html>
    "#,
        body_html
    );

    println!("{html}");

    dioxus_native::launch_static_html_cfg(
        &html,
        Config {
            stylesheets: vec![String::from(stylesheet)],
            base_url: Some(base_url),
        },
    );
}

#[allow(unused)]
struct CustomSyntectAdapter(SyntectAdapter);

impl SyntaxHighlighterAdapter for CustomSyntectAdapter {
    fn write_highlighted(
        &self,
        output: &mut dyn std::io::Write,
        lang: Option<&str>,
        code: &str,
    ) -> std::io::Result<()> {
        let norm_lang = lang.map(|l| l.split_once(',').map(|(lang, _)| lang).unwrap_or(l));
        self.0.write_highlighted(output, norm_lang, code)
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn std::io::Write,
        attributes: HashMap<String, String>,
    ) -> std::io::Result<()> {
        self.0.write_pre_tag(output, attributes)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn std::io::Write,
        attributes: HashMap<String, String>,
    ) -> std::io::Result<()> {
        self.0.write_code_tag(output, attributes)
    }
}
