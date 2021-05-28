use std::{fs::File, io::BufReader, path::Path};

use quick_xml::events::{BytesEnd, BytesStart, Event};

pub enum Error {
    XmlError {
        context: &'static str,
        error: quick_xml::Error,
    },
    Schema {
        context: &'static str,
    },
    UnexpectedEvent {
        event: Event<'static>,
    },
    ExpectedTag {
        tag: &'static [u8],
        actual: Vec<u8>,
    },
}

impl Error {
    fn xml_with_context(error: quick_xml::Error, context: &'static str) -> Self {
        Error::XmlError { context, error }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct MediaWikiXmlReader {
    reader: quick_xml::Reader<BufReader<File>>,
    buf: Vec<u8>,
}

impl MediaWikiXmlReader {
    pub fn from_file(path: &Path) -> std::result::Result<Self, quick_xml::Error> {
        Ok(Self {
            reader: quick_xml::Reader::from_file(path)?,
            buf: Vec::new(),
        })
    }

    pub fn read(&mut self) -> Result<()> {
        self.read_base()?;
        self.read_siteinfo()?;
        Ok(())
    }

    fn read_event(&mut self) -> std::result::Result<Event, quick_xml::Error> {
        self.reader.read_event(&mut self.buf)
    }

    fn read_event_with_context(&mut self, context: &'static str) -> std::result::Result<Event, Error> {
        self.reader.read_event(&mut self.buf).map_err(|e| {
            Error::xml_with_context(e, context)
        })
    }

    fn clear_buffer(&mut self) {
        self.buf.clear();
    }

    fn tag_start<'a>(&'a mut self) -> Result<BytesStart<'a>> {
        match self.read_event() {
            Ok(Event::Start(start)) => Ok(start),
            Ok(event) => Err(Error::UnexpectedEvent { event: event.into_owned() }),
            Err(e) => Err(Error::xml_with_context(e.into(), "tag start")),
        }
    }

    fn tag_end<'a>(&'a mut self) -> Result<BytesEnd<'a>> {
        match self.read_event() {
            Ok(Event::End(end)) => Ok(end),
            Ok(event) => Err(Error::UnexpectedEvent { event: event.into_owned() }),
            Err(e) => Err(Error::xml_with_context(e.into(), "tag end")),
        }
    }

    fn text<'a>(&'a mut self) -> Result<Vec<u8>> {
        match self.read_event() {
            Ok(Event::Text(text)) => {
                Ok(text.unescaped().map_err(|error| {
                    Error::xml_with_context(error, "unescaping text")
                })?.into())
            }
            Ok(event) => Err(Error::UnexpectedEvent { event: event.into_owned() }),
            Err(e) => Err(Error::xml_with_context(e.into(), "text")),
        }
    }

    fn read_base(&mut self) -> Result<()> {
        let start = self.tag_start()?;
        let tag = b"mediawiki";
        // TODO: Check attributes?
        if start.name() != tag {
            Err(Error::ExpectedTag { tag, actual: start.name().into() })
        } else {
            assert!(matches!(self.read_event(), Ok(Event::Text(_))));
            Ok(())
        }
    }

    /*
    fn with_optional_tag<'a, T>(&'a mut self, prev_event: &mut Option<Event<'a>>, tag: &'static [u8], mut f: impl FnMut(BytesText) -> T) -> Result<()> {
        let start = if let Some(event) = prev_event {
            event
        } else {
            &mut self.read_event_with_context("optional tag")?
        };
        // let start = 
        //         if let Event::Start(start) = prev_event.unwrap_or_else(|| self.read_event_with_context("optional tag")).transpose()? {
        //             if start.name() == tag {
        //                 event
        //             } else {
        //                 return Err(Error::ExpectedTag {
        //                     tag,
        //                     actual: start.name().into(),
        //                 })
        //             }
        //         } else {
        //             *prev_event = Some(event);
        //             return Ok(())
        //         };
        if start.name() == tag {
            let event = self.read_event().map_err(|error| {
                Error::xml_with_context(error, "read optional tag")
            })?;
            match event {
                Event::Text(text) => {
                    f(text);
                    let end = self.tag_end()?;
                    if end.name() != tag {
                        Err(Error::ExpectedTag {
                            tag,
                            actual: end.name().into(),
                        })
                    } else {
                        Ok(())
                    }
                }
                _ => Err(Error::UnexpectedEvent { event: event.into_owned() })
            }
        } else {
            Err(Error::ExpectedTag {
                tag,
                actual: start.name().into(),
            })
        }
    }
    */

    fn read_siteinfo(&mut self) -> Result<()> {
        Ok(())
    }

    /*
    fn read_base(&mut self) -> Result<(), Error> {
        match self.reader.read_event(&mut self.buf) {
            Ok(Event::Start(start)) => {
                let attributes: Vec<_> = start
                    .attributes()
                    .map(|res| res.map(|Attribute { key, value }| (key, value)))
                    .collect();
                if start.name() == b"mediawiki" && start.attributes().find_map(|res| {
                    match res {
                        Ok(Attribute { key, value }) => {
                            if key == b"xsi:schemaLocation" && value.as_ref() == b"http://www.mediawiki.org/xml/export-0.10/ http://www.mediawiki.org/xml/export-0.10.xsd".as_ref() {
                                Some(Ok(true))
                            } else {
                                None
                            }
                        }
                        Err(e) => {
                            Some(Err(e))
                        }
                    }
                }).transpose().map_err(|error| Error::xml_with_context(error, "failed to parse attributes in mediawiki element"))? == Some(true) {
                    Ok(())
                } else {
                    Err(Error::Schema { context: "missing or incorrect xsi:schemaLocation in mediawiki tag"})
                }
            }
            Ok(event) => Err(Error::UnexpectedEvent { event }),
            Err(error) => Err(Error::xml_with_context(error, "base element")),
        }
        // self.read_siteinfo()
    }
    */
}
