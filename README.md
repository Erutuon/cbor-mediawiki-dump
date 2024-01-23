# cbor-mediawiki-dump
The library crate provides functions (`parse_from_file` and `parse`) that parse the XML dumps of Wikimedia pages (for instance, `pages-articles.xml.bz2`).
The binary crate converts the page information into formats that are easier to parse than XML:
[CBOR](https://cbor.io/) sequence, [JSONL](https://jsonlines.org/), [Bincode](https://docs.rs/bincode/),
[MessagePack](https://msgpack.org/).

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
