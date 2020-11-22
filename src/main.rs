use std::{convert::TryFrom, io::BufReader, fs::File, path::PathBuf};

use bzip2::read::BzDecoder;
use lzma::reader::LzmaReader;

fn main() -> anyhow::Result<()> {
    let mut args = pico_args::Arguments::from_env();
    let pages_xml_path = args
        .opt_value_from_os_str(["-f", "--file"], |p| PathBuf::try_from(p))?
        .unwrap_or_else(|| "pages-articles.xml".into());
    let file =File::open(&pages_xml_path)?;
    if pages_xml_path.extension() == Some("bz2".as_ref()) {
        cbor_mediawiki_dump::parse(BufReader::new(BzDecoder::new(file)))?;
    } else if pages_xml_path.extension() == Some("7z".as_ref()) {
        cbor_mediawiki_dump::parse(BufReader::new(LzmaReader::new_decompressor(file)?))?;
    } else {
        cbor_mediawiki_dump::parse(BufReader::new(file))?;
    }
    Ok(())
}
