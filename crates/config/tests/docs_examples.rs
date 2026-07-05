//! Every KDL example in the documentation site must parse.
//!
//! Walks `docs/src/content/docs/` for Markdown and MDX pages, extracts every
//! fenced code block whose info string starts with `kdl`, and feeds it to
//! [`miditool_config::parse_str`]. Blocks fenced as `kdl fail` document
//! invalid configs and are asserted to NOT parse; everything else must
//! parse cleanly. Failures report the file, the block's line, and the
//! parse error, so a broken example is easy to find.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// One fenced `kdl` block, located well enough to point a human at it.
struct Block {
    file: PathBuf,
    line: usize,
    text: String,
    /// The block was fenced `kdl fail`: it documents a config that must
    /// be rejected.
    expect_fail: bool,
}

#[test]
fn docs_examples_parse() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/src/content/docs");
    let docs = docs
        .canonicalize()
        .unwrap_or_else(|e| panic!("docs content directory {}: {e}", docs.display()));

    let mut pages = Vec::new();
    collect_pages(&docs, &mut pages);
    assert!(
        !pages.is_empty(),
        "no .md/.mdx pages under {}",
        docs.display()
    );
    pages.sort();

    let mut blocks = Vec::new();
    for page in &pages {
        extract_kdl_blocks(page, &mut blocks);
    }
    assert!(
        !blocks.is_empty(),
        "no ```kdl blocks found in {} pages; the extractor or the docs are broken",
        pages.len()
    );

    let mut report = String::new();
    let mut bad = 0usize;
    for block in &blocks {
        let result = miditool_config::parse_str(&block.file.display().to_string(), &block.text);
        match (block.expect_fail, result) {
            (false, Ok(_)) | (true, Err(_)) => {}
            (false, Err(e)) => {
                bad += 1;
                let _ = writeln!(
                    report,
                    "{}:{}: example does not parse:\n{e}\n",
                    block.file.display(),
                    block.line
                );
            }
            (true, Ok(_)) => {
                bad += 1;
                let _ = writeln!(
                    report,
                    "{}:{}: block is fenced `kdl fail` but parses fine:\n{}\n",
                    block.file.display(),
                    block.line,
                    block.text
                );
            }
        }
    }
    assert!(
        bad == 0,
        "{bad} of {} documented KDL examples misbehave:\n\n{report}",
        blocks.len()
    );
}

/// Every `.md`/`.mdx` file under `dir`, recursively.
fn collect_pages(dir: &Path, pages: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect_pages(&path, pages);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md") | Some("mdx")
        ) {
            pages.push(path);
        }
    }
}

/// Pull every fenced block whose info string's first word is `kdl` out of
/// one page. `kdl fail` (the second word) marks a block that documents a
/// config the parser must reject; further words (`title="..."` and other
/// code-frame metadata) are ignored.
fn extract_kdl_blocks(page: &Path, blocks: &mut Vec<Block>) {
    let source =
        fs::read_to_string(page).unwrap_or_else(|e| panic!("read {}: {e}", page.display()));
    let parser = Parser::new_ext(&source, Options::empty()).into_offset_iter();

    let mut current: Option<Block> = None;
    for (event, range) in parser {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let mut words = info.split_whitespace();
                if words.next() != Some("kdl") {
                    continue;
                }
                let expect_fail = words.next() == Some("fail");
                current = Some(Block {
                    file: page.to_path_buf(),
                    line: source[..range.start].lines().count() + 1,
                    text: String::new(),
                    expect_fail,
                });
            }
            Event::Text(text) => {
                if let Some(block) = &mut current {
                    block.text.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
            }
            _ => {}
        }
    }
}
