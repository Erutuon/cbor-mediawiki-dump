use std::{
    borrow::Cow,
    convert::{Infallible, TryFrom},
    fs::File,
    io::{BufRead, BufReader},
    net::IpAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

#[cfg(feature = "bz2")]
use bzip2::read::BzDecoder;
use chrono::{DateTime, Utc};
#[cfg(feature = "lzma")]
use lzma::{LzmaError, LzmaReader};
use memchr::memmem;
use quick_xml::{events::Event, name::QName, Reader};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod tag;
use tag::Tag;

#[derive(Error, Debug)]
pub enum Error<E: std::error::Error + 'static = Infallible> {
    #[error("invalid XML (schema or format) at position {position}")]
    Format { position: usize },
    #[error(
        "expected tag {}, got tag {} at position {position}",
        expected.as_str(),
        actual.as_str(),
    )]
    Tag {
        expected: Tag,
        actual: Tag,
        position: usize,
    },
    #[error("failed to unescape or decode UTF-8 at position {position}")]
    FailedToDecode { position: usize },
    #[error("failed to open XML file: {0}")]
    File(#[source] quick_xml::Error),
    #[error("Failed to {action} at {}", path.display())]
    Io {
        action: &'static str,
        source: std::io::Error,
        path: PathBuf,
    },
    #[error("Failed to decode LZMA at {}", path.display())]
    #[cfg(feature = "lzma")]
    Lzma { source: LzmaError, path: Box<Path> },
    #[error("Unexpected tag: {}", String::from_utf8_lossy(.0))]
    UnexpectedTag(Box<[u8]>),
    /// Return `Err(Error::ShortCircuit)` from the `page_processor` callback of [`parse`] or [`parse_from_file`]
    /// to stop parsing pages early even though there was no error.
    #[error("Done deserializing")]
    ShortCircuit,
    /// This error variant allows the `page_processor` callback of [`parse`] or [`parse_from_file`].
    /// to indicate that there was an error in parsing. Set it to `std::convert::Infallible`
    /// if your callback cannot fail.
    #[error("Error in page processor")]
    Other(#[source] E),
}

impl<E: std::error::Error> Error<E> {
    fn format<R: BufRead>(reader: &Reader<R>) -> Self {
        Self::Format {
            position: reader.buffer_position(),
        }
    }

    fn tag<R: BufRead>(reader: &Reader<R>, expected: Tag, actual: Tag) -> Self {
        Self::Tag {
            expected,
            actual,
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

    /// This is used to convert internal errors,
    /// which do not use the `Error::Other(_)` variant,
    /// into the error type chosen by the caller of [`parse`].
    #[allow(clippy::wrong_self_convention)]
    fn from_infallible(e: Error<Infallible>) -> Error<E> {
        match e {
            Error::Format { position } => Error::Format { position },
            Error::Tag {
                expected,
                actual: tag,
                position,
            } => Error::Tag {
                expected,
                actual: tag,
                position,
            },
            Error::FailedToDecode { position } => Error::FailedToDecode { position },
            Error::File(e) => Error::File(e),
            Error::ShortCircuit => Error::ShortCircuit,
            Error::Io {
                action,
                source,
                path,
            } => Error::Io {
                action,
                source,
                path,
            },
            #[cfg(feature = "lzma")]
            Error::Lzma { source, path } => Error::Lzma { source, path },
            Error::UnexpectedTag(e) => Error::UnexpectedTag(e),
            Error::Other(_) => unreachable!(),
        }
    }

    pub fn to_infallible(e: Error<E>) -> Result<Error<Infallible>, E> {
        Ok(match e {
            Error::Format { position } => Error::Format { position },
            Error::Tag {
                expected,
                actual: tag,
                position,
            } => Error::Tag {
                expected,
                actual: tag,
                position,
            },
            Error::FailedToDecode { position } => Error::FailedToDecode { position },
            Error::File(e) => Error::File(e),
            Error::ShortCircuit => Error::ShortCircuit,
            Error::Io {
                action,
                source,
                path,
            } => Error::Io {
                action,
                source,
                path,
            },
            #[cfg(feature = "lzma")]
            Error::Lzma { source, path } => Error::Lzma { source, path },
            Error::UnexpectedTag(e) => Error::UnexpectedTag(e),
            Error::Other(other) => return Err(other),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct Page {
    pub title: String,
    pub namespace: i32,
    pub id: u32,
    pub redirect_target: Option<String>,
    pub restrictions: Option<String>,
    pub revisions: Vec<Revision>,
}

#[derive(Serialize, Deserialize)]
pub struct Revision {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub timestamp: DateTime<Utc>,
    pub contributor: Contributor,
    pub origin: u32,
    pub minor: bool,
    pub comment: Comment,
    pub model: String,  // Could be converted to integer using hashmap.
    pub format: String, // Could be converted to integer using hashmap.
    pub text: String,
    pub sha1: String,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum Comment {
    DeletedOrAbsent(bool),
    Visible(String),
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
// #[serde(untagged)]
pub enum Contributor {
    Deleted,
    Ip { ip: IpAddr },
    User { username: String, id: u32 },
}

#[test]
fn test_contributor_deserialize() {
    #[track_caller]
    fn assert_round_trip<T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug + Eq>(
        val: T,
    ) {
        let cbor = serde_cbor::to_vec(&val).unwrap();
        assert_eq!(serde_cbor::from_slice::<T>(&cbor).unwrap(), val);
    }
    assert_round_trip(Contributor::Deleted);
    assert_round_trip(Contributor::User {
        username: "Wonderfool".into(),
        id: 1,
    });
    assert_round_trip(Contributor::Ip {
        ip: std::net::IpAddr::from([127, 0, 0, 1]),
    });
}

fn get_start_tag<R: BufRead, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
) -> Result<(Tag, bool), Error<E>> {
    match reader.read_event_into(buf) {
        Ok(Event::Start(start)) => Ok((
            Tag::try_from(start.name()).map_err(Error::from_infallible)?,
            false,
        )),
        Ok(Event::Empty(start)) => Ok((
            Tag::try_from(start.name()).map_err(Error::from_infallible)?,
            true,
        )),
        _ => Err(Error::format(reader)),
    }
}

#[allow(clippy::type_complexity)]
fn get_start_tag_and_attribute<R: BufRead, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
) -> Result<(Tag, Option<(Vec<u8>, String)>, bool), Error<E>> {
    let event = reader.read_event_into(buf);
    let (tag, is_empty) = match &event {
        Ok(Event::Start(start)) => (start, false),
        Ok(Event::Empty(start)) => (start, true),
        _ => return Err(Error::format(reader)),
    };
    let key_value = tag
        .attributes()
        .next()
        .map(|attr_result| {
            let attr = attr_result.map_err(|_| Error::format(reader))?;
            if let Ok(value) = std::str::from_utf8(&attr.value).map(String::from) {
                Ok((attr.key.as_ref().to_vec(), value))
            } else {
                Err(Error::format(reader))
            }
        })
        .transpose()?;
    Ok((
        Tag::try_from(tag.name()).map_err(Error::from_infallible)?,
        key_value,
        is_empty,
    ))
}

fn expect_tag_start<R: BufRead, E: std::error::Error>(
    reader: &Reader<R>,
    event: &Event,
    expected_tag: Tag,
) -> Result<(), Error<E>> {
    let (Event::Start(start) | Event::Empty(start)) = event else {
        return Err(Error::format(reader));
    };
    let tag = Tag::try_from(start.name()).map_err(Error::from_infallible)?;
    if tag == expected_tag {
        Ok(())
    } else {
        Err(Error::tag(reader, expected_tag, tag))
    }
}

fn expect_tag_start_from_reader<'a, 'b, R: BufRead, E: std::error::Error>(
    reader: &'a mut Reader<R>,
    buf: &'b mut Vec<u8>,
    expected_tag: Tag,
) -> Result<Event<'b>, Error<E>> {
    let event = reader
        .read_event_into(buf)
        .map_err(|_| Error::format(reader))?;
    expect_tag_start(&reader, &event, expected_tag).map(|_| event)
}

fn expect_tag_end<R: BufRead, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
    expected_tag: Tag,
) -> Result<(), Error<E>> {
    let Event::End(end) = reader
        .read_event_into(buf)
        .map_err(|_| Error::format(reader))?
    else {
        return Err(Error::format(reader));
    };
    let tag = Tag::try_from(end.name()).map_err(Error::from_infallible)?;
    if tag == expected_tag {
        Ok(())
    } else {
        Err(Error::tag(reader, tag, expected_tag))
    }
}

pub fn skip_text<R: BufRead, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
) -> Result<(), Error<E>> {
    let text = reader
        .read_event_into(buf)
        .map_err(|_| Error::format(reader))?;
    if matches!(text, Event::Text(_)) {
        Ok(())
    } else {
        Err(Error::format(reader))
    }
}

fn map_unescaped_text<
    R: BufRead,
    T,
    E: std::error::Error,
    F: FnMut(Cow<'_, str>) -> Result<T, Error<E>>,
>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
    expected_tag: Tag,
    mut f: F,
) -> Result<T, Error<E>> {
    match reader
        .read_event_into(buf)
        .map_err(|_| Error::format(reader))?
    {
        Event::Text(text) => {
            let text = text.unescape().map_err(|_| Error::FailedToDecode {
                position: reader.buffer_position(),
            })?;
            let res = f(text);
            let Event::End(end) = reader
                .read_event_into(buf)
                .map_err(|_| Error::format(reader))?
            else {
                return Err(Error::format(reader));
            };
            let tag = Tag::try_from(end.name()).map_err(Error::from_infallible)?;
            if tag == expected_tag {
                res
            } else {
                Err(Error::tag(reader, expected_tag, tag))
            }
        }
        _ => Err(Error::format(reader)),
    }
}

fn parse_text<R: BufRead, T: FromStr, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
    tag: Tag,
) -> Result<T, Error<E>> {
    let position = reader.buffer_position();
    map_unescaped_text(reader, buf, tag, |text| {
        text.as_ref()
            .parse()
            .map_err(|_| Error::Format { position })
    })
}

fn read_text<R: BufRead, E: std::error::Error>(
    reader: &mut Reader<R>,
    buf: &mut Vec<u8>,
    tag: Tag,
) -> Result<String, Error<E>> {
    let text = if let Event::Text(t) = reader
        .read_event_into(buf)
        .map_err(|_| Error::format(reader))?
    {
        t.unescape().map_err(|_| Error::format(reader))?.into()
    } else {
        return Err(Error::format(reader));
    };
    if let Event::End(name) = reader
        .read_event_into(buf)
        .map_err(|_| Error::format(reader))?
    {
        if name.name() == tag.as_q_name() {
            Ok(text)
        } else {
            Err(Error::format(reader))
        }
    } else {
        Err(Error::format(reader))
    }
}

// Search for <page> containing <title> with given title.
pub fn find_page(title_to_find: &str, xml: &[u8]) -> Result<Option<Page>, Error<Infallible>> {
    // quick_xml::escape::escape can't be used because it escapes ' to &apos;,
    // but the MediaWiki dump has literal apostrophes.
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

    // skip_text(&mut reader, &mut buf)?;
    // Skip over initial mediawiki tag.
    if skip_header {
        expect_tag_start_from_reader(&mut reader, &mut buf, Tag::MediaWiki)?;
        skip_text(&mut reader, &mut buf)?;
        expect_tag_start_from_reader(&mut reader, &mut buf, Tag::SiteInfo)?;
        reader
            .read_to_end_into(QName(b"siteinfo"), &mut buf)
            .map_err(|_| Error::format(&reader))?;
        skip_text(&mut reader, &mut buf)?;
    }
    buf.clear();

    // let stdout = std::io::stdout();
    // let mut stdout = stdout.lock();

    let mut restrictions_buffer = Vec::<u8>::new();

    // page elements
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(start)) if start.name() == QName(b"page") => (),
            Ok(Event::End(end)) if end.name() == QName(b"mediawiki") => return Ok(()),
            _ => return Err(Error::format(&reader)),
        }
        skip_text(&mut reader, &mut buf)?;

        expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Title)?;
        let title = read_text(&mut reader, &mut buf, Tag::Title)?;
        skip_text(&mut reader, &mut buf)?;

        expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Ns)?;
        let namespace: i32 = parse_text(&mut reader, &mut buf, Tag::Ns)?;
        skip_text(&mut reader, &mut buf)?;

        expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Id)?;
        let id: u32 = parse_text(&mut reader, &mut buf, Tag::Id)?;
        skip_text(&mut reader, &mut buf)?;

        let (tag_start, attribute, is_empty) = get_start_tag_and_attribute(&mut reader, &mut buf)?;
        let ((tag_start, _), redirect_target) = {
            if tag_start == Tag::Redirect {
                if !is_empty {
                    return Err(Error::format(&reader));
                }

                if let Some((_, title)) = attribute {
                    skip_text(&mut reader, &mut buf)?;
                    (get_start_tag(&mut reader, &mut buf)?, Some(title))
                } else {
                    return Err(Error::format(&reader));
                }
            } else {
                ((tag_start, is_empty), None)
            }
        };

        let restrictions = {
            if tag_start == Tag::Restrictions {
                Some(read_text(&mut reader, &mut buf, Tag::Restrictions)?)
            } else if tag_start == Tag::Revision {
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
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(start)) if start.name() == QName(b"revision") => {
                        skip_text(&mut reader, &mut buf)?;
                    }
                    Ok(Event::End(end)) if end.name() == QName(b"page") => {
                        skip_text(&mut reader, &mut buf)?;
                        break;
                    }
                    _ => return Err(Error::format(&reader)),
                }
            } else {
                read_revision_start = true;
            }

            expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Id)?;
            let id: u32 = parse_text(&mut reader, &mut buf, Tag::Id)?;
            skip_text(&mut reader, &mut buf)?;

            let (tag, _) = get_start_tag(&mut reader, &mut buf)?;
            let (tag, parent_id) = if tag == Tag::ParentId {
                let parent_id = parse_text(&mut reader, &mut buf, Tag::ParentId)?;
                skip_text(&mut reader, &mut buf)?;
                let (tag, _) = get_start_tag(&mut reader, &mut buf)?;
                (tag, Some(parent_id))
            } else {
                (tag, None)
            };

            if tag != Tag::Timestamp {
                return Err(Error::format(&reader));
            }
            let timestamp = parse_text(&mut reader, &mut buf, Tag::Timestamp)?;
            skip_text(&mut reader, &mut buf)?;

            let contributor = {
                let (tag, attribute, is_empty) =
                    get_start_tag_and_attribute(&mut reader, &mut buf)?;
                if tag != Tag::Contributor {
                    return Err(Error::format(&reader));
                }
                if is_empty {
                    if let Some((key, value)) = attribute {
                        if key == b"deleted" && value.as_bytes() == b"deleted" {
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
                    let contributor = if tag == Tag::Username {
                        let username = read_text(&mut reader, &mut buf, Tag::Username)?;
                        skip_text(&mut reader, &mut buf)?;

                        expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Id)?;
                        let id: u32 = parse_text(&mut reader, &mut buf, Tag::Id)?;
                        skip_text(&mut reader, &mut buf)?;
                        Contributor::User { username, id }
                    } else if tag == Tag::Ip {
                        let ip = parse_text(&mut reader, &mut buf, Tag::Ip)?;
                        skip_text(&mut reader, &mut buf)?;
                        Contributor::Ip { ip }
                    } else {
                        return Err(Error::format(&reader));
                    };

                    expect_tag_end(&mut reader, &mut buf, Tag::Contributor)?;

                    contributor
                }
            };
            skip_text(&mut reader, &mut buf)?;
            dbg!();

            let event = reader
                .read_event_into(&mut buf)
                .map_err(|_| Error::format(&reader))?;
            let (event, minor) = if let Event::Empty(empty) = &event {
                if empty.name() == QName(b"minor") {
                    skip_text(&mut reader, &mut buf)?;
                    (
                        reader
                            .read_event_into(&mut buf)
                            .map_err(|_| Error::format(&reader))?,
                        true,
                    )
                } else {
                    (event, false)
                }
            } else {
                (event, false)
            };

            expect_tag_start(&reader, &event, Tag::Origin)?;
            dbg!();
            let origin: u32 = parse_text(&mut reader, &mut buf, Tag::Origin)?;
            dbg!();
            skip_text(&mut reader, &mut buf)?;
            dbg!(origin);

            let event = expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Comment)?;
            let (event, comment) = if let Event::Start(start) = &event {
                if start.name() == QName(b"comment") {
                    let comment = parse_text(&mut reader, &mut buf, Tag::Comment)?;
                    skip_text(&mut reader, &mut buf)?;
                    (
                        reader
                            .read_event_into(&mut buf)
                            .map_err(|_| Error::format(&reader))?,
                        Comment::Visible(comment),
                    )
                } else {
                    (event, Comment::DeletedOrAbsent(false))
                }
            } else if let Event::Empty(empty) = &event {
                if empty.name() == QName(b"comment") {
                    let mut attributes = empty.attributes();
                    if let (Some(Ok(attr)), None) = (attributes.next(), attributes.next()) {
                        if attr.key == QName(b"deleted") && attr.value.as_ref() == b"deleted" {
                            skip_text(&mut reader, &mut buf)?;
                            (
                                reader
                                    .read_event_into(&mut buf)
                                    .map_err(|_| Error::format(&reader))?,
                                Comment::DeletedOrAbsent(true),
                            )
                        } else {
                            dbg!(&event);
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

            expect_tag_start(&reader, &event, Tag::Model)?;
            let model = parse_text(&mut reader, &mut buf, Tag::Model)?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Format)?;
            let format = parse_text(&mut reader, &mut buf, Tag::Format)?;
            skip_text(&mut reader, &mut buf)?;

            let (tag, is_empty) = get_start_tag(&mut reader, &mut buf)?;
            if tag != Tag::Text {
                return Err(Error::format(&reader));
            }
            let text = if is_empty {
                String::new()
            } else {
                parse_text(&mut reader, &mut buf, Tag::Text)?
            };
            skip_text(&mut reader, &mut buf)?;

            expect_tag_start_from_reader(&mut reader, &mut buf, Tag::Sha1)?;
            let sha1 = parse_text(&mut reader, &mut buf, Tag::Sha1)?;
            skip_text(&mut reader, &mut buf)?;

            expect_tag_end(&mut reader, &mut buf, Tag::Revision)?;
            skip_text(&mut reader, &mut buf)?;

            revisions.push(Revision {
                id,
                parent_id,
                timestamp,
                contributor,
                origin,
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
        #[cfg(feature = "bz2")]
        Some("bz2") => parse(
            BufReader::new(BzDecoder::new(file)),
            page_processor,
            skip_header,
        ),
        #[cfg(feature = "lzma")]
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
