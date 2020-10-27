use std::{fs::File, io::BufReader};

use serde::Serialize;

#[derive(Serialize)]
pub struct Page<N> {
    pub namespace: N,
    pub title: String,
    pub format: Option<String>,
    pub model: Option<String>,
    pub redirect_title: Option<String>,
    pub text: String,
}

impl From<dump_parser::Page> for Page<i32> {
    fn from(page: dump_parser::Page) -> Self {
        let dump_parser::Page { namespace, title, model, format, redirect_title, text } = page;
        let namespace = i32::from(namespace);
        Page { namespace, title, model, format, text, redirect_title }
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let pages_xml = args.next().unwrap_or_else(|| "pages-articles.xml".into());
    let tuple = args.next() == Some("tuple".into());
    let file = File::open(&pages_xml)?;
    let file = BufReader::new(file);
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    if tuple {
        for page in dump_parser::parse(file) {
            if let Ok(dump_parser::Page { namespace, title, format, model, redirect_title, text }) = page {
                serde_cbor::to_writer(&mut stdout, &(i32::from(namespace), title, format, model, redirect_title, text))?;
            }
        }
    } else {
        for page in dump_parser::parse(file) {
            if let Ok(page) = page {
                serde_cbor::to_writer(&mut stdout, &Page::from(page))?;
            }
        }
    }
    Ok(())
}
