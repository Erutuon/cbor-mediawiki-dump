use std::{convert::TryFrom, path::PathBuf};

use cbor_mediawiki_dump::{Error, parse_from_file};

fn main() -> anyhow::Result<()> {
    let mut args = pico_args::Arguments::from_env();
    #[allow(clippy::redundant_closure)]
    let pages_xml_path = args
        .opt_value_from_os_str(["-f", "--file"], |p| PathBuf::try_from(p))?
        .unwrap_or_else(|| "pages-articles.xml".into());

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    parse_from_file(
        pages_xml_path,
        |page| serde_cbor::to_writer(&mut stdout, &page).map_err(Error::Other),
        true,
    )?;

    Ok(())
}
