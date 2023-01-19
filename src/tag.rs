use std::{convert::TryFrom, str::FromStr};

use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tag {
    Action,
    Base,
    Case,
    Comment,
    Contributor,
    DbName,
    DiscussionThreadingInfo,
    Filename,
    Format,
    Generator,
    Id,
    Ip,
    LogItem,
    LogTitle,
    MediaWiki,
    Minor,
    Model,
    Namespace,
    Namespaces,
    Ns,
    Page,
    Params,
    ParentId,
    Redirect,
    Restrictions,
    Revision,
    Sha1,
    SiteInfo,
    SiteName,
    Size,
    Src,
    Text,
    ThreadAncestor,
    ThreadAuthor,
    ThreadEditStatus,
    ThreadId,
    ThreadPage,
    ThreadParent,
    ThreadSubject,
    ThreadType,
    Timestamp,
    Title,
    Type,
    Upload,
    Username,
}

impl Tag {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        use Tag::*;
        let tag = match self {
            Action => "action",
            Base => "base",
            Case => "case",
            Comment => "comment",
            Contributor => "contributor",
            DbName => "dbname",
            DiscussionThreadingInfo => "discussionthreadinginfo",
            Filename => "filename",
            Format => "format",
            Generator => "generator",
            Id => "id",
            Ip => "ip",
            LogItem => "logitem",
            LogTitle => "logtitle",
            MediaWiki => "mediawiki",
            Minor => "minor",
            Model => "model",
            Namespace => "namespace",
            Namespaces => "namespaces",
            Ns => "ns",
            Page => "page",
            Params => "params",
            ParentId => "parentid",
            Redirect => "redirect",
            Restrictions => "restrictions",
            Revision => "revision",
            Sha1 => "sha1",
            SiteInfo => "siteinfo",
            SiteName => "sitename",
            Size => "size",
            Src => "src",
            Text => "text",
            ThreadAncestor => "ThreadAncestor",
            ThreadAuthor => "ThreadAuthor",
            ThreadEditStatus => "ThreadEditStatus",
            ThreadId => "ThreadID",
            ThreadPage => "ThreadPage",
            ThreadParent => "ThreadParent",
            ThreadSubject => "ThreadSubject",
            ThreadType => "ThreadType",
            Timestamp => "timestamp",
            Title => "title",
            Type => "type",
            Upload => "upload",
            Username => "username",
        };
        tag.as_bytes()
    }
}

impl FromStr for Tag {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Tag::*;
        let tag = match s {
            "action" => Action,
            "base" => Base,
            "case" => Case,
            "comment" => Comment,
            "contributor" => Contributor,
            "dbname" => DbName,
            "discussionthreadinginfo" => DiscussionThreadingInfo,
            "filename" => Filename,
            "format" => Format,
            "generator" => Generator,
            "id" => Id,
            "ip" => Ip,
            "logitem" => LogItem,
            "logtitle" => LogTitle,
            "mediawiki" => MediaWiki,
            "minor" => Minor,
            "model" => Model,
            "namespace" => Namespace,
            "namespaces" => Namespaces,
            "ns" => Ns,
            "page" => Page,
            "params" => Params,
            "parentid" => ParentId,
            "redirect" => Redirect,
            "restrictions" => Restrictions,
            "revision" => Revision,
            "sha1" => Sha1,
            "siteinfo" => SiteInfo,
            "sitename" => SiteName,
            "size" => Size,
            "src" => Src,
            "text" => Text,
            "ThreadAncestor" => ThreadAncestor,
            "ThreadAuthor" => ThreadAuthor,
            "ThreadEditStatus" => ThreadEditStatus,
            "ThreadID" => ThreadId,
            "ThreadPage" => ThreadPage,
            "ThreadParent" => ThreadParent,
            "ThreadSubject" => ThreadSubject,
            "ThreadType" => ThreadType,
            "timestamp" => Timestamp,
            "title" => Title,
            "type" => Type,
            "upload" => Upload,
            "username" => Username,
            _ => return Err(Error::UnexpectedTag(s.as_bytes().into())),
        };
        Ok(tag)
    }
}

impl TryFrom<&[u8]> for Tag {
    type Error = Error;

    fn try_from(s: &[u8]) -> Result<Self, Self::Error> {
        std::str::from_utf8(s)
            .map_err(|_| Error::UnexpectedTag(s.into()))?
            .parse()
    }
}
