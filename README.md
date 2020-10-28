# cbor-mediawiki-dump
Converts XML dumps of MediaWiki pages to a [CBOR](https://cbor.io/) sequence of page objects
containing the same page information.

Usage: decompress `pages-articles.xml` or `pages-meta-history.xml` for the Wikimedia project
that you are interested in, and run

    cargo run --release -- xml-dump-path-here > cbor-file-name-here
