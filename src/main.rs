use std::{convert::TryFrom, fs::File, io::BufReader, path::PathBuf};

use bzip2::read::BzDecoder;
use lzma::reader::LzmaReader;

fn main() -> anyhow::Result<()> {
    let mut args = pico_args::Arguments::from_env();
    let pages_xml_path = args
        .opt_value_from_os_str(["-f", "--file"], |p| PathBuf::try_from(p))?
        .unwrap_or_else(|| "pages-articles.xml".into());
    let file = File::open(&pages_xml_path)?;
    match pages_xml_path.extension().and_then(|s| s.to_str()) {
        Some("bz2") => cbor_mediawiki_dump::parse(BufReader::new(BzDecoder::new(file)))?,
        Some("7z") => {
            cbor_mediawiki_dump::parse(BufReader::new(LzmaReader::new_decompressor(file)?))?
        }
        _ => cbor_mediawiki_dump::parse(BufReader::new(file))?,
    }
    Ok(())
}
