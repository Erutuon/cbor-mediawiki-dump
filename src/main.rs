use std::{convert::TryFrom, path::PathBuf, str::FromStr};

use cbor_mediawiki_dump::{parse_from_file, Error};

enum Format {
    Cbor,
    Bincode,
    Jsonl,
    MessagePack,
}

impl FromStr for Format {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s.eq_ignore_ascii_case("cbor") {
            Self::Cbor
        } else if s.eq_ignore_ascii_case("bincode") {
            Self::Bincode
        } else if s.eq_ignore_ascii_case("messagepack") {
            Self::MessagePack
        } else if s.eq_ignore_ascii_case("json") || s.eq_ignore_ascii_case("jsonl") {
            Self::Jsonl
        } else {
            return Err("Invalid format");
        })
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = pico_args::Arguments::from_env();
    #[allow(clippy::redundant_closure)]
    let pages_xml_path = args
        .opt_value_from_os_str(["-f", "--file"], |p| PathBuf::try_from(p))?
        .unwrap_or_else(|| "pages-articles.xml".into());
    let format: Format = args
        .opt_value_from_str(["-F", "--format"])?
        .unwrap_or(Format::Cbor);

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    match format {
        Format::Cbor => {
            parse_from_file(
                &pages_xml_path,
                |page| serde_cbor::to_writer(&mut stdout, &page).map_err(Error::Other),
                true,
            )?;
        }
        Format::Bincode => {
            use bincode::Options;
            let options = bincode::DefaultOptions::new()
                .with_fixint_encoding()
                .allow_trailing_bytes();
            parse_from_file(
                &pages_xml_path,
                |page| options.serialize_into(&mut stdout, &page).map_err(Error::Other),
                true,
            )?;
        }
        Format::Jsonl => {
            parse_from_file(
                &pages_xml_path,
                |page| {
                    use std::io::Write;
                    use either::Either;
                    serde_json::to_writer(&mut stdout, &page).map_err(|e| Error::Other(Either::Right(e)))?;
                    writeln!(&mut stdout).map_err(|e| Error::Other(Either::Left(e)))
                },
                true,
            )?;
        }
        Format::MessagePack => {
            use serde::Serialize as _;
            let mut serializer = rmp_serde::encode::Serializer::new(&mut stdout);
            parse_from_file(
                &pages_xml_path,
                |page| page.serialize(&mut serializer).map_err(Error::Other),
                true,
            )?;
        }
    }

    Ok(())
}
