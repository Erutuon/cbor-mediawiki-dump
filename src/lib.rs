use std::{borrow::Cow, fs::File, io::BufReader};

use chrono::{DateTime, Utc};
use serde::Serialize;
use quick_xml::events::Event;

type Reader = quick_xml::Reader<BufReader<File>>;

pub enum Error {
    Format { position: usize },
    FailedToDecode { position: usize },
}

impl Error {
    fn format(reader: &Reader) -> Self {
        Self::Format {
            position: reader.buffer_position(),
        }
    }
}

#[derive(Serialize)]
pub struct Page {
    title: String,
    namespace: i32,
    id: u32,
    redirect_target: Option<String>,
    restrictions: String,
    revisions: Vec<Revision>,
}

#[derive(Serialize)]
pub struct Revision {
    id: u32,
    parent_id: u32,
    timestamp: DateTime<Utc>,
    contributor: Contributor,
    comment: String,
    model: String, // Could be converted to integer using hashmap.
    format: String, // Could be converted to integer using hashmap.
    text: String,
    sha1: String,
}

#[derive(Serialize)]
pub struct Contributor {
    username: String,
    id: u32,
}

pub fn expect_tag_start(tag: &str, reader: &mut Reader, buf: &mut Vec<u8>) -> Result<(), Error> {
    if !matches!(reader.read_event(buf).unwrap(), Event::Start(start) if start.name() != tag.as_bytes())
    {
        Err(Error::format(reader))
    } else {
        Ok(())
    }
}

pub fn expect_tag_end(tag: &str, reader: &mut Reader, buf: &mut Vec<u8>) -> Result<(), Error> {
    if !matches!(reader.read_event(buf).unwrap(), Event::End(end) if end.name() != tag.as_bytes())
    {
        Err(Error::format(reader))
    } else {
        Ok(())
    }
}

pub fn skip_text(reader: &mut Reader, buf: &mut Vec<u8>) -> Result<(), Error> {
    if !matches!(reader.read_event(buf).unwrap(), Event::Text(_)) {
        return Err(Error::format(reader));
    }
    Ok(())
}

pub fn map_unescaped_text<F: FnMut(Cow<'_, [u8]>) -> Result<T, Error>, T>(
    reader: &mut Reader,
    buf: &mut Vec<u8>,
    tag: &str,
    mut f: F,
) -> Result<T, Error> {
    expect_tag_start(tag, reader, buf)?;
    match reader.read_event(buf).map_err(|_| Error::format(reader))? {
        Event::Text(text) => {
            let text = text.unescaped().map_err(|_| Error::FailedToDecode {
                position: reader.buffer_position(),
            })?;
            let res = f(text);
            if !matches!(reader.read_event(buf), Ok(Event::End(end)) if end.name() == tag.as_bytes())
            {
                Err(Error::format(reader))
            } else {
                res
            }
        }
        _ => Err(Error::format(reader)),
    }
}

pub fn parse() -> Result<(), Error> {
    // Bigger than maximum revision length (2 MiB).
    let mut buf = Vec::with_capacity(3 * 1024 * 1024);
    let mut reader = Reader::from_file("pages-articles.xml").unwrap();
    expect_tag_start("mediawiki", &mut reader, &mut buf)?;
    skip_text(&mut reader, &mut buf)?;
    expect_tag_start("siteinfo", &mut reader, &mut buf)?;
    reader
        .read_to_end("siteinfo", &mut buf)
        .map_err(|_| Error::format(&reader))?;
    skip_text(&mut reader, &mut buf)?;
    buf.clear();

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    let mut restrictions_buffer = Vec::new();

    // page elements
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(start)) if start.name() == b"page" => (),
            Ok(Event::End(end)) if end.name() == b"mediawiki" => return Ok(()),
            _ => return Err(Error::format(&reader)),
        }
        expect_tag_start("title", &mut reader, &mut buf)?;
        let title = reader
            .read_text("title", &mut buf)
            .map_err(|_| Error::format(&reader))?;
        skip_text(&mut reader, &mut buf)?;
        let position = reader.buffer_position();
        let namespace: i32 = map_unescaped_text(&mut reader, &mut buf, "ns", |ns| {
            std::str::from_utf8(ns.as_ref())
                .map_err(|_| Error::Format { position })?
                .parse()
                .map_err(|_| Error::Format { position })
        })?;
        skip_text(&mut reader, &mut buf)?;
        let position = reader.buffer_position();
        let id: u32 = map_unescaped_text(&mut reader, &mut buf, "id", |ns| {
            std::str::from_utf8(ns.as_ref())
                .map_err(|_| Error::Format { position })?
                .parse()
                .map_err(|_| Error::Format { position })
        })?;
        skip_text(&mut reader, &mut buf)?;
        // TODO: Read optional redirect target.
        let position = reader.buffer_position();
        let restrictions = map_unescaped_text(
            &mut reader,
            &mut restrictions_buffer,
            "restrictions",
            |restrictions| {
                std::str::from_utf8(restrictions.as_ref())
                    .map(String::from)
                    .map_err(|_| Error::Format { position })
            },
        )?;
        skip_text(&mut reader, &mut buf)?;

        // revision elements
        let mut revisions = Vec::new();
        loop {
            buf.clear();
            match reader.read_event(&mut buf) {
                Ok(Event::Start(start)) if start.name() == b"revision" => (),
                Ok(Event::End(end)) if end.name() == b"page" => break,
                _ => return Err(Error::format(&reader)),
            }
            skip_text(&mut reader, &mut buf)?;
            let id: u32 = map_unescaped_text(&mut reader, &mut buf, "id", |ns| {
                std::str::from_utf8(ns.as_ref())
                    .map_err(|_| Error::Format { position })?
                    .parse()
                    .map_err(|_| Error::Format { position })
            })?;
            skip_text(&mut reader, &mut buf)?;
            let parent_id: u32 = map_unescaped_text(&mut reader, &mut buf, "parentid", |ns| {
                std::str::from_utf8(ns.as_ref())
                    .map_err(|_| Error::Format { position })?
                    .parse()
                    .map_err(|_| Error::Format { position })
            })?;
            skip_text(&mut reader, &mut buf)?;
            let timestamp: DateTime<Utc> =
                map_unescaped_text(&mut reader, &mut buf, "parentid", |ns| {
                    DateTime::parse_from_rfc3339(
                        std::str::from_utf8(ns.as_ref()).map_err(|_| Error::Format { position })?,
                    )
                    .map(DateTime::<Utc>::from)
                    .map_err(|_| Error::Format { position })
                })?;
            skip_text(&mut reader, &mut buf)?;
            let contributor = {
                expect_tag_start("contributor", &mut reader, &mut buf)?;
                expect_tag_start("username", &mut reader, &mut buf)?;
                let username = reader.read_text("username", &mut buf).map_err(|_| Error::Format { position })?;
                let id: u32 = map_unescaped_text(&mut reader, &mut buf, "parentid", |ns| {
                    std::str::from_utf8(ns.as_ref())
                        .map_err(|_| Error::Format { position })?
                        .parse()
                        .map_err(|_| Error::Format { position })
                })?;
                skip_text(&mut reader, &mut buf)?;
                expect_tag_start("contributor", &mut reader, &mut buf)?;
                Contributor {
                    username,
                    id,
                }
            };
            skip_text(&mut reader, &mut buf)?;
            expect_tag_start("comment", &mut reader, &mut buf)?;
            let comment = reader.read_text("comment", &mut buf).map_err(|_| Error::Format { position })?;
            skip_text(&mut reader, &mut buf)?;
            expect_tag_start("model", &mut reader, &mut buf)?;
            let model = reader.read_text("model", &mut buf).map_err(|_| Error::Format { position })?;
            skip_text(&mut reader, &mut buf)?;
            expect_tag_start("format", &mut reader, &mut buf)?;
            let format = reader.read_text("format", &mut buf).map_err(|_| Error::Format { position })?;
            skip_text(&mut reader, &mut buf)?;
            expect_tag_start("text", &mut reader, &mut buf)?;
            let text = reader.read_text("text", &mut buf).map_err(|_| Error::Format { position })?;
            skip_text(&mut reader, &mut buf)?;
            expect_tag_start("sha1", &mut reader, &mut buf)?;
            let sha1 = reader
                .read_text("sha1", &mut buf)
                .map_err(|_| Error::format(&reader))?;
            skip_text(&mut reader, &mut buf)?;
            let revision = Revision {
                id,
                parent_id,
                timestamp,
                contributor,
                comment,
                model,
                format,
                text,
                sha1,
                
            };
            revisions.push(revision);
        }
        let page = Page {
            title,
            namespace,
            id,
            redirect_target: None,
            restrictions,
            revisions: Vec::new(),
        };

        serde_cbor::to_writer(&mut stdout, &page).expect("failed to write CBOR to stdout");
        
        restrictions_buffer.clear();
    }
}
