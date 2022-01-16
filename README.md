# cbor-mediawiki-dump
Converts XML dumps of MediaWiki pages to a [CBOR](https://cbor.io/) sequence of page objects
containing the same page information.

# Usage
Download an XML dump, such as `pages-articles.xml.bz2` or `pages-meta-current.xml.bz2`, for the Wikimedia project
that you are interested in, and run this command on the file, whether compressed (`.xml.bz2` or `.xml.7z`) or decompressed to `.xml`:

    cargo run --release -- --file xml-dump-path-here > cbor-file-name-here

Other formats are supported (JSONL, MessagePack, Bincode). For JSONL:

    cargo run --release -- --file xml-dump-path-here --format jsonl > cbor-file-name-here

The resulting file contains all the fields in the XML. The format isn't documented,
but it is fairly straightforward to figure out from the JSONL.

# Features
`.xml.bz2` requires the `bz2` feature and `.xml.7z` requires the `7z` feature.
Both are enabled by the `decompress` feature.
