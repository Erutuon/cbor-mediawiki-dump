fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let pages_xml = args.next().unwrap_or_else(|| "pages-articles.xml".into());
    cbor_mediawiki_dump::parse(pages_xml.as_ref())?;
    Ok(())
}
