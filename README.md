# cbor-mediawiki-dump
Converts XML dumps of MediaWiki pages to a [CBOR](https://cbor.io/) sequence of page objects
containing the same page information.

Usage: download `pages-articles.xml` or `pages-meta-current.xml` for the Wikimedia project
that you are interested in, and run this command on the file, compressed (`.xml.bz2` or `.xml.7z`) or uncompressed (`.xml`):

    cargo run --release -- --file xml-dump-path-here > cbor-file-name-here

Other formats are supported (JSONL, MessagePack, Bincode). For JSONL:

    cargo run --release -- --file xml-dump-path-here --format jsonl > cbor-file-name-here

The resulting file contains all the fields in the XML. The format isn't documented,
but it is fairly straightforward to figure out from the JSONL.
