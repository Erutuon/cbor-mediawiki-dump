use std::{borrow::Cow, fs::File, io::BufReader, net::IpAddr, path::Path};

use chrono::{DateTime, Utc};
use quick_xml::events::{BytesStart, Event};
use serde::Serialize;
use thiserror::Error;

type Reader = quick_xml::Reader<BufReader<File>>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("invalid XML (schema or format) at position {position}")]
    Format { position: usize },
    #[error("failed to unescape or decode UTF-8 at position {position}")]
    FailedToDecode { position: usize },
    #[error("failed to open XML file: {0}")]
    File(quick_xml::Error),
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
    restrictions: Option<String>,
    revisions: Vec<Revision>,
}

#[derive(Serialize)]
pub struct Revision {
    id: u32,
    parent_id: u32,
    timestamp: DateTime<Utc>,
    contributor: Contributor,
    minor: bool,
    comment: Option<String>,
    model: String,  // Could be converted to integer using hashmap.
    format: String, // Could be converted to integer using hashmap.
    text: String,
    sha1: String,
}

#[derive(Serialize)]
pub enum Contributor {
    User {
        username: String,
        id: u32,
    },
    Ip {
        address: IpAddr,
    }
}

pub fn get_tag_name<'a>(
    reader: &mut Reader,
    buf: &'a mut Vec<u8>,
) -> Result<BytesStart<'a>, Error> {
    match reader.read_event(buf) {
        Ok(Event::Start(start)) | Ok(Event::Empty(start)) => Ok(start),
        _ => Err(Error::format(reader)),
    }
}

pub fn expect_tag_start(tag: &str, reader: &mut Reader, buf: &mut Vec<u8>) -> Result<(), Error> {
    let start = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&start);
    if matches!(start, Event::Start(start) if start.name() == tag.as_bytes()) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn expect_tag_end(tag: &str, reader: &mut Reader, buf: &mut Vec<u8>) -> Result<(), Error> {
    let end = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&end);
    if matches!(end, Event::End(end) if end.name() == tag.as_bytes()) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn skip_text(reader: &mut Reader, buf: &mut Vec<u8>) -> Result<(), Error> {
    let text = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&text);
    if matches!(text, Event::Text(_)) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
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

pub fn parse(path: &Path) -> Result<(), Error> {
    // Bigger than maximum revision length (2 MiB).
    let mut buf = Vec::with_capacity(3 * 1024 * 1024);
    let mut reader = Reader::from_file(path).map_err(|e| Error::File(e))?;

    skip_text(&mut reader, &mut buf)?;
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
        skip_text(&mut reader, &mut buf)?;

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
        let id: u32 = map_unescaped_text(&mut reader, &mut buf, "id", |id| {
            std::str::from_utf8(id.as_ref())
                .map_err(|_| Error::Format { position })?
                .parse()
                .map_err(|_| Error::Format { position })
        })?;
        skip_text(&mut reader, &mut buf)?;

        let tag_start = get_tag_name(&mut reader, &mut buf)?;
        let (tag_start, redirect_target) = {
            if tag_start.name() == b"redirect" {
                if let Some(title) = tag_start.attributes().find_map(|attr| {
                    if let Ok(attr) = attr {
                        Some(String::from_utf8(attr.value.to_vec()).map_err(|_| {
                            Error::FailedToDecode {
                                position: reader.buffer_position(),
                            }
                        }))
                    } else {
                        None
                    }
                }) {
                    reader
                        .read_to_end("redirect", &mut buf)
                        .map_err(|_| Error::format(&reader))?;
                    skip_text(&mut reader, &mut buf)?;
                    (get_tag_name(&mut reader, &mut buf)?, Some(title?))
                } else {
                    (tag_start, None)
                }
            } else {
                (tag_start, None)
            }
        };

        let restrictions = {
            if tag_start.name() == b"restrictions" {
                Some(
                    reader
                        .read_text("restrictions", &mut restrictions_buffer)
                        .map_err(|_| Error::format(&reader))?,
                )
            } else if tag_start.name() == b"revision" {
                None
            } else {
                return Err(Error::format(&reader));
            }
        };
        skip_text(&mut reader, &mut buf)?;

        let mut read_revision_start = restrictions.is_some();

        // revision elements
        let mut revisions = Vec::new();
        loop {
            buf.clear();
            if read_revision_start {
                match reader.read_event(&mut buf) {
                    Ok(Event::Start(start)) if start.name() == b"revision" => {
                        skip_text(&mut reader, &mut buf)?;
                    }
                    Ok(Event::End(end)) if end.name() == b"page" => {
                        skip_text(&mut reader, &mut buf)?;
                        break;
                    }
                    _ => return Err(Error::format(&reader)),
                }
            } else {
                read_revision_start = true;
            }

            let id: u32 = map_unescaped_text(&mut reader, &mut buf, "id", |id| {
                std::str::from_utf8(id.as_ref())
                    .map_err(|_| Error::Format { position })?
                    .parse()
                    .map_err(|_| Error::Format { position })
            })?;
            skip_text(&mut reader, &mut buf)?;

            let parent_id: u32 =
                map_unescaped_text(&mut reader, &mut buf, "parentid", |parent_id| {
                    std::str::from_utf8(parent_id.as_ref())
                        .map_err(|_| Error::Format { position })?
                        .parse()
                        .map_err(|_| Error::Format { position })
                })?;
            skip_text(&mut reader, &mut buf)?;

            let timestamp: DateTime<Utc> =
                map_unescaped_text(&mut reader, &mut buf, "timestamp", |timestamp| {
                    DateTime::parse_from_rfc3339(
                        std::str::from_utf8(timestamp.as_ref())
                            .map_err(|_| Error::Format { position })?,
                    )
                    .map(DateTime::<Utc>::from)
                    .map_err(|_| Error::Format { position })
                })?;
            skip_text(&mut reader, &mut buf)?;

            let contributor = {
                expect_tag_start("contributor", &mut reader, &mut buf)?;
                skip_text(&mut reader, &mut buf)?;

                let tag = get_tag_name(&mut reader, &mut buf)?;
                let contributor = if tag.name() == b"username" {
                    let username = reader
                        .read_text("username", &mut buf)
                        .map_err(|_| Error::format(&reader))?;
                    skip_text(&mut reader, &mut buf)?;

                    let position = reader.buffer_position();
                    let id: u32 = map_unescaped_text(&mut reader, &mut buf, "id", |id| {
                        std::str::from_utf8(id.as_ref())
                            .map_err(|_| Error::Format { position })?
                            .parse()
                            .map_err(|_| Error::Format { position })
                    })?;
                    skip_text(&mut reader, &mut buf)?;
                    Contributor::User { username, id }
                } else if tag.name() == b"ip" {
                    let address = reader
                        .read_text("ip", &mut buf)
                        .map_err(|_| Error::format(&reader))
                        .and_then(|text| text.parse().map_err(|_| Error::format(&reader)))?;
                    skip_text(&mut reader, &mut buf)?;
                    Contributor::Ip { address }
                } else {
                    return Err(Error::format(&reader));
                };

                expect_tag_end("contributor", &mut reader, &mut buf)?;

                contributor
            };
            skip_text(&mut reader, &mut buf)?;

            let event = reader.read_event(&mut buf).map_err(|_| Error::format(&reader))?;
            let (event, minor) = if let Event::Empty(empty) = event {
                if empty.name() == b"minor" {
                    skip_text(&mut reader, &mut buf)?;
                    (reader.read_event(&mut buf).map_err(|_| Error::format(&reader))?, true)
                } else {
                    return Err(Error::format(&reader));
                }
            } else {
                (event, false)
            };

            let (event, comment) = if let Event::Start(start) = &event {
                if start.name() == b"comment" {
                    let comment = reader
                        .read_text("comment", &mut buf)
                        .map_err(|_| Error::Format { position })?;
                    skip_text(&mut reader, &mut buf)?;
                    (reader.read_event(&mut buf).map_err(|_| Error::format(&reader))?, Some(comment))
                } else {
                    (event, None)
                }
            } else {
                return Err(Error::format(&reader));
            };

            if let Event::Start(start) = event {
                if start.name() != b"model" {
                    return Err(Error::format(&reader));
                }
            } else {
                return Err(Error::format(&reader));
            }
            let model = reader
                .read_text("model", &mut buf)
                .map_err(|_| Error::Format { position })?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_start("format", &mut reader, &mut buf)?;
            let format = reader
                .read_text("format", &mut buf)
                .map_err(|_| Error::Format { position })?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_start("text", &mut reader, &mut buf)?;
            let text = reader
                .read_text("text", &mut buf)
                .map_err(|_| Error::Format { position })?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_start("sha1", &mut reader, &mut buf)?;
            let sha1 = reader
                .read_text("sha1", &mut buf)
                .map_err(|_| Error::format(&reader))?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_end("revision", &mut reader, &mut buf)?;
            skip_text(&mut reader, &mut buf)?;

            let revision = Revision {
                id,
                parent_id,
                timestamp,
                contributor,
                minor,
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
            redirect_target,
            restrictions,
            revisions,
        };

        serde_cbor::to_writer(&mut stdout, &page).expect("failed to write CBOR to stdout");

        restrictions_buffer.clear();
    }
}
