use std::{
    borrow::Cow,
    convert::Infallible,
    fs::File,
    io::{BufRead, BufReader},
    net::IpAddr,
    path::{Path, PathBuf},
};

use bzip2::read::BzDecoder;
use chrono::{DateTime, Utc};
use lzma::{LzmaError, LzmaReader};
use memchr::memmem;
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error<E: std::error::Error = Infallible> {
    #[error("invalid XML (schema or format) at position {position}")]
    Format { position: usize },
    #[error("failed to unescape or decode UTF-8 at position {position}")]
    FailedToDecode { position: usize },
    #[error("failed to open XML file: {0}")]
    File(quick_xml::Error),
    #[error("Done deserializing")]
    ShortCircuit,
    #[error("Failed to {action} at {}", path.display())]
    Io {
        action: &'static str,
        source: std::io::Error,
        path: PathBuf,
    },
    #[error("Failed to decode LZMA at {}", path.display())]
    Lzma { source: LzmaError, path: PathBuf },
    #[error("{0}")]
    Other(E),
}

impl<E: std::error::Error> Error<E> {
    fn format<R: BufRead>(reader: &Reader<R>) -> Self {
        Self::Format {
            position: reader.buffer_position(),
        }
    }

    fn from_io<P: Into<PathBuf>>(action: &'static str, source: std::io::Error, path: P) -> Self {
        Error::Io {
            action,
            source,
            path: path.into(),
        }
    }
}

#[derive(Serialize)]
pub struct Page {
    pub title: String,
    pub namespace: i32,
    pub id: u32,
    pub redirect_target: Option<String>,
    pub restrictions: Option<String>,
    pub revisions: Vec<Revision>,
}

#[derive(Serialize)]
pub struct Revision {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub timestamp: DateTime<Utc>,
    pub contributor: Contributor,
    pub minor: bool,
    pub comment: Comment,
    pub model: String,  // Could be converted to integer using hashmap.
    pub format: String, // Could be converted to integer using hashmap.
    pub text: String,
    pub sha1: String,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum Comment {
    DeletedOrAbsent(bool),
    Visible(String),
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum Contributor {
    Deleted,
    Ip { ip: IpAddr },
    User { username: String, id: u32 },
}

pub fn get_start_tag<'a, R: BufRead, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &'a mut Vec<u8>,
) -> Result<(BytesStart<'a>, bool), Error<E>> {
    match reader.read_event(buf) {
        Ok(Event::Start(start)) => Ok((start, false)),
        Ok(Event::Empty(start)) => Ok((start, true)),
        _ => Err(Error::format(reader)),
    }
}

pub fn expect_tag_start<R: BufRead, E: std::error::Error>(
    tag: &str,
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
) -> Result<(), Error<E>> {
    let start = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&start);
    if matches!(start, Event::Start(start) if start.name() == tag.as_bytes()) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn expect_tag_end<R: BufRead, E: std::error::Error>(
    tag: &str,
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
) -> Result<(), Error<E>> {
    let end = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&end);
    if matches!(end, Event::End(end) if end.name() == tag.as_bytes()) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn skip_text<R: BufRead, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
) -> Result<(), Error<E>> {
    let text = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&text);
    if matches!(text, Event::Text(_)) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn map_unescaped_text<
    R: BufRead,
    T,
    E: std::error::Error,
    F: FnMut(Cow<'_, [u8]>) -> Result<T, Error<E>>,
>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
    tag: &str,
    mut f: F,
) -> Result<T, Error<E>> {
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

// Search for <page> containing <title> with given title.
pub fn find_page(title_to_find: &str, xml: &[u8]) -> Result<Option<Page>, Error<Infallible>> {
    let title_tag = format!(
        "<title>{}</title>",
        title_to_find
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt")
    );
    if let Some(title_tag_index) = memmem::find(xml, title_tag.as_ref()) {
        if let Some(page_tag_index) = memmem::rfind(&xml[..title_tag_index], b"<page>") {
            let mut found_page = None;
            parse(
                &xml[page_tag_index..],
                |page| {
                    if page.title == title_to_find {
                        found_page = Some(page);
                    }
                    Err(Error::ShortCircuit)
                },
                false,
            )?;
            return Ok(found_page);
        }
    }
    Ok(None)
}

pub fn parse<R: BufRead, F: FnMut(Page) -> Result<(), Error<E>>, E: std::error::Error>(
    reader: R,
    mut page_processor: F,
    skip_header: bool,
) -> Result<(), Error<E>> {
    // Bigger than maximum revision length (2 MiB).
    let mut buf = Vec::with_capacity(3 * 1024 * 1024);
    let mut reader = Reader::from_reader(reader);

    skip_text(&mut reader, &mut buf)?;
    // Skip over initial mediawiki tag.
    if skip_header {
        expect_tag_start("mediawiki", &mut reader, &mut buf)?;
        skip_text(&mut reader, &mut buf)?;
        expect_tag_start("siteinfo", &mut reader, &mut buf)?;
        reader
            .read_to_end("siteinfo", &mut buf)
            .map_err(|_| Error::format(&reader))?;
        skip_text(&mut reader, &mut buf)?;
    }
    buf.clear();

    // let stdout = std::io::stdout();
    // let mut stdout = stdout.lock();

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

        let (tag_start, is_empty) = get_start_tag(&mut reader, &mut buf)?;
        let ((tag_start, _), redirect_target) = {
            if tag_start.name() == b"redirect" {
                if !is_empty {
                    return Err(Error::format(&reader));
                }

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
                    skip_text(&mut reader, &mut buf)?;
                    (get_start_tag(&mut reader, &mut buf)?, Some(title?))
                } else {
                    return Err(Error::format(&reader));
                }
            } else {
                ((tag_start, is_empty), None)
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

            let position = reader.buffer_position();
            let id: u32 = map_unescaped_text(&mut reader, &mut buf, "id", |id| {
                std::str::from_utf8(id.as_ref())
                    .map_err(|_| Error::Format { position })?
                    .parse()
                    .map_err(|_| Error::Format { position })
            })?;
            skip_text(&mut reader, &mut buf)?;

            let (tag, _) = get_start_tag(&mut reader, &mut buf)?;
            let (tag, parent_id) = if tag.name() == b"parentid" {
                let parent_id = reader
                    .read_text("parentid", &mut buf)
                    .map_err(|_| Error::format(&reader))?
                    .parse()
                    .map_err(|_| Error::format(&reader))?;
                skip_text(&mut reader, &mut buf)?;
                let (tag, _) = get_start_tag(&mut reader, &mut buf)?;
                (tag, Some(parent_id))
            } else {
                (tag, None)
            };

            if tag.name() != b"timestamp" {
                return Err(Error::format(&reader));
            }
            let timestamp = reader
                .read_text("timestamp", &mut buf)
                .map_err(|_| Error::format(&reader))?
                .parse()
                .map_err(|_| Error::format(&reader))?;
            skip_text(&mut reader, &mut buf)?;

            let contributor = {
                let (tag, is_empty) = get_start_tag(&mut reader, &mut buf)?;
                if tag.name() != b"contributor" {
                    return Err(Error::format(&reader));
                }
                if is_empty {
                    let mut attributes = tag.attributes();
                    if let (Some(Ok(attr)), None) = (attributes.next(), attributes.next()) {
                        if attr.key == b"deleted" && attr.value.as_ref() == b"deleted" {
                            Contributor::Deleted
                        } else {
                            return Err(Error::format(&reader));
                        }
                    } else {
                        return Err(Error::format(&reader));
                    }
                } else {
                    skip_text(&mut reader, &mut buf)?;

                    let (tag, _) = get_start_tag(&mut reader, &mut buf)?;
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
                        let ip = reader
                            .read_text("ip", &mut buf)
                            .map_err(|_| Error::format(&reader))
                            .and_then(|text| text.parse().map_err(|_| Error::format(&reader)))?;
                        skip_text(&mut reader, &mut buf)?;
                        Contributor::Ip { ip }
                    } else {
                        return Err(Error::format(&reader));
                    };

                    expect_tag_end("contributor", &mut reader, &mut buf)?;

                    contributor
                }
            };
            skip_text(&mut reader, &mut buf)?;

            let event = reader
                .read_event(&mut buf)
                .map_err(|_| Error::format(&reader))?;
            let (event, minor) = if let Event::Empty(empty) = &event {
                if empty.name() == b"minor" {
                    skip_text(&mut reader, &mut buf)?;
                    (
                        reader
                            .read_event(&mut buf)
                            .map_err(|_| Error::format(&reader))?,
                        true,
                    )
                } else {
                    (event, false)
                }
            } else {
                (event, false)
            };

            let (event, comment) = if let Event::Start(start) = &event {
                if start.name() == b"comment" {
                    let comment = reader
                        .read_text("comment", &mut buf)
                        .map_err(|_| Error::format(&reader))?;
                    skip_text(&mut reader, &mut buf)?;
                    (
                        reader
                            .read_event(&mut buf)
                            .map_err(|_| Error::format(&reader))?,
                        Comment::Visible(comment),
                    )
                } else {
                    (event, Comment::DeletedOrAbsent(false))
                }
            } else if let Event::Empty(empty) = &event {
                if empty.name() == b"comment" {
                    let mut attributes = empty.attributes();
                    if let (Some(Ok(attr)), None) = (attributes.next(), attributes.next()) {
                        if attr.key == b"deleted" && attr.value.as_ref() == b"deleted" {
                            skip_text(&mut reader, &mut buf)?;
                            (
                                reader
                                    .read_event(&mut buf)
                                    .map_err(|_| Error::format(&reader))?,
                                Comment::DeletedOrAbsent(true),
                            )
                        } else {
                            return Err(Error::format(&reader));
                        }
                    } else {
                        return Err(Error::format(&reader));
                    }
                } else {
                    return Err(Error::format(&reader));
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
                .map_err(|_| Error::format(&reader))?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_start("format", &mut reader, &mut buf)?;
            let format = reader
                .read_text("format", &mut buf)
                .map_err(|_| Error::format(&reader))?;
            skip_text(&mut reader, &mut buf)?;

            let (tag, is_empty) = get_start_tag(&mut reader, &mut buf)?;
            if tag.name() != b"text" {
                return Err(Error::format(&reader));
            }
            let text = if is_empty {
                String::new()
            } else {
                reader
                    .read_text("text", &mut buf)
                    .map_err(|_| Error::format(&reader))?
            };
            skip_text(&mut reader, &mut buf)?;

            expect_tag_start("sha1", &mut reader, &mut buf)?;
            let sha1 = reader
                .read_text("sha1", &mut buf)
                .map_err(|_| Error::format(&reader))?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_end("revision", &mut reader, &mut buf)?;
            skip_text(&mut reader, &mut buf)?;

            revisions.push(Revision {
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
            });
        }

        match page_processor(Page {
            title,
            namespace,
            id,
            redirect_target,
            restrictions,
            revisions,
        }) {
            Err(Error::ShortCircuit) => return Ok(()),
            Err(e) => return Err(e),
            _ => {}
        }
        // serde_cbor::to_writer(&mut stdout, &page).expect("failed to write CBOR to stdout");

        restrictions_buffer.clear();
    }
}

pub fn parse_from_file<
    P: AsRef<Path>,
    F: FnMut(Page) -> Result<(), Error<E>>,
    E: std::error::Error,
>(
    path: P,
    page_processor: F,
    skip_header: bool,
) -> Result<(), Error<E>> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|e| Error::from_io("open", e, path))?;

    match path.extension().and_then(|s| s.to_str()) {
        Some("bz2") => parse(
            BufReader::new(BzDecoder::new(file)),
            page_processor,
            skip_header,
        ),
        Some("7z") => parse(
            BufReader::new(
                LzmaReader::new_decompressor(file).map_err(|source| Error::Lzma {
                    source,
                    path: path.into(),
                })?,
            ),
            page_processor,
            skip_header,
        ),
        _ => parse(BufReader::new(file), page_processor, skip_header),
    }
}
