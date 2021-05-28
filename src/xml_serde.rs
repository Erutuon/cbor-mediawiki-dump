use serde::Deserialize;
use url::Url;

/// Top level tag of MediaWiki dump.
#[derive(Deserialize)]
struct MediaWiki {
    #[serde(rename = "xml:lang")]
    lang: String,
    version: String,
    site_info: Option<SiteInfo>,
    #[serde(rename = "page", default)]
    pages: Vec<Page>,
}

#[derive(Deserialize)]
struct SiteInfo {
    #[serde(rename = "sitename")]
    site_name: Option<String>,
    #[serde(rename = "dbname")]
    database_name: Option<String>,
    base: Option<String>,
    generator: Option<String>,
    case: Option<Case>,

}

#[derive(Deserialize)]
enum Case {
    #[serde(rename = "first-letter")]
    FirstLetter,
    #[serde(rename = "case-sensitive")]
    CaseSensitive,
    #[serde(rename = "case-insensitive")]
    CaseInsensitive,
}

#[derive(Deserialize)]
struct Page {

}