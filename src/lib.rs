use std::{borrow::Cow, io::BufRead, net::IpAddr};

use chrono::{DateTime, Utc};
use quick_xml::{Reader, events::{BytesStart, Event}};
use serde::Serialize;
use thiserror::Error;

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
    fn format<R: BufRead>(reader: &Reader<R>) -> Self {
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
    parent_id: Option<u32>,
    timestamp: DateTime<Utc>,
    contributor: Contributor,
    minor: bool,
    comment: Comment,
    model: String,  // Could be converted to integer using hashmap.
    format: String, // Could be converted to integer using hashmap.
    text: String,
    sha1: String,
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

pub fn get_start_tag<'a, R: BufRead>(
    reader: &mut Reader<R>,
    buf: &'a mut Vec<u8>,
) -> Result<(BytesStart<'a>, bool), Error> {
    match reader.read_event(buf) {
        Ok(Event::Start(start)) => Ok((start, false)),
        Ok(Event::Empty(start)) => Ok((start, true)),
        _ => Err(Error::format(reader)),
    }
}

pub fn expect_tag_start<R: BufRead>(tag: &str, reader: &mut Reader<R>, buf: &mut Vec<u8>) -> Result<(), Error> {
    let start = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&start);
    if matches!(start, Event::Start(start) if start.name() == tag.as_bytes()) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn expect_tag_end<R: BufRead>(tag: &str, reader: &mut Reader<R>, buf: &mut Vec<u8>) -> Result<(), Error> {
    let end = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&end);
    if matches!(end, Event::End(end) if end.name() == tag.as_bytes()) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn skip_text<R: BufRead>(reader: &mut Reader<R>, buf: &mut Vec<u8>) -> Result<(), Error> {
    let text = reader.read_event(buf).map_err(|_| Error::format(reader))?;
    // dbg!(&text);
    if matches!(text, Event::Text(_)) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

pub fn map_unescaped_text<R: BufRead, F: FnMut(Cow<'_, [u8]>) -> Result<T, Error>, T>(
    reader: &mut Reader<R>,
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

pub fn parse<R: BufRead>(reader: R) -> Result<(), Error> {
    // Bigger than maximum revision length (2 MiB).
    let mut buf = Vec::with_capacity(3 * 1024 * 1024);
    let mut reader = Reader::from_reader(reader);

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
