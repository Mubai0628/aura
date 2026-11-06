//! What a reply becomes on its channel before it is sent: Markdown turned into plain text where
//! the platform cannot show it, and split into the pieces the platform takes at once, at a blank
//! line, a line break, the end of a sentence or a space before a hard cut; a code block cut between
//! two pieces is closed at the end of the first and opened again at the start of the next.
use aura_plugin_host::{ChannelAbilities, ReplyFormat};
use aura_plugin_session::{outgoing, plain_text, split_text};

#[test]
fn a_long_text_is_split_where_it_reads_best() {
    assert_eq!(split_text("a\n\nb\nc", 5), ["a", "b\nc"]);
    assert_eq!(split_text("ab\ncde", 4), ["ab", "cde"]);
    assert_eq!(split_text("你好。我是谁啊", 4), ["你好。", "我是谁啊"]);
    assert_eq!(split_text("hello world foo", 11), ["hello", "world foo"]);
    assert_eq!(split_text("abcdefgh", 3), ["abc", "def", "gh"]);
    assert_eq!(split_text("😀😀😀", 2), ["😀😀", "😀"]);
    assert_eq!(split_text("", 5), [""]);
    assert_eq!(split_text("短的", 5), ["短的"]);
    // A line's indentation stays; nothing is empty, and a cut leaves no line break behind.
    assert_eq!(split_text("x\n\n\n  y", 3), ["x", "  y"]);
    assert_eq!(split_text("abcde\n\nf", 5), ["abcde", "f"]);
    // No piece is only blanks, a cut leaves no space behind either, and a sentence's end counts
    // only after the piece's first character.
    assert_eq!(split_text("a\n\n  \nbbbb", 4), ["a", "bbbb"]);
    assert_eq!(split_text("abcd\n   ", 4), ["abcd"]);
    assert_eq!(split_text("a\n\n\nbcd", 4), ["a", "bcd"]);
    assert_eq!(split_text("ab  cd ef", 5), ["ab", "cd ef"]);
    assert_eq!(split_text("。abcdefgh", 4), ["。abc", "defg", "h"]);
}

#[test]
fn markdown_becomes_plain_text_for_a_channel_that_cannot_show_it() {
    let markdown = "# 标题\n\n**粗体**和`**kwargs**`，~~删掉~~，见[文档](https://example.com)、\
                    ![图](https://example.com/a.png)、[https://a.dev](https://a.dev)、<https://b.dev>\n\n\
                    ```python\nprint(\"**\")\n```\n- 列表\n__init__ 不动";
    assert_eq!(
        plain_text(markdown),
        "标题\n\n粗体和**kwargs**，删掉，见文档（https://example.com）、图（https://example.com/a.png）、\
         https://a.dev、https://b.dev\n\nprint(\"**\")\n- 列表\n__init__ 不动"
    );
    assert_eq!(plain_text("没有格式的一句话。"), "没有格式的一句话。");
    // An address with brackets in it, a link's title, and brackets that make no link.
    assert_eq!(
        plain_text(
            "[Rust](https://en.wikipedia.org/wiki/Rust_(language) \"标题\")，[a] b [c](https://d.dev)"
        ),
        "Rust（https://en.wikipedia.org/wiki/Rust_(language)），[a] b c（https://d.dev）"
    );
    // A block ends only at its own fence, alone on its line.
    assert_eq!(plain_text("````md\n```\n````\n~~~\n```python\n~~~"), "```\n```python");
    assert_eq!(plain_text("```\n```python\n```"), "```python");
    // Two backticks are inline code, and a `#` without a space after it is no heading.
    assert_eq!(plain_text("``x``"), "x");
    assert_eq!(plain_text("#话题 不是标题"), "#话题 不是标题");
}

fn channel(max_text_chars: u32, markdown: bool) -> ChannelAbilities {
    ChannelAbilities {
        id: "qq-main".into(),
        max_text_chars,
        markdown,
        buttons: false,
        command_menu: false,
        groups: false,
    }
}

#[test]
fn a_reply_is_shaped_by_what_its_channel_reported() {
    let text = "**好**\n\n第二段";
    let markdown = |piece: &str| (piece.to_owned(), ReplyFormat::Markdown);
    let plain = |piece: &str| (piece.to_owned(), ReplyFormat::Text);
    let rich = channel(5, true);
    assert_eq!(
        outgoing(text, ReplyFormat::Markdown, Some(&rich)),
        [markdown("**好**"), markdown("第二段")]
    );
    let bare = channel(5, false);
    assert_eq!(outgoing(text, ReplyFormat::Markdown, Some(&bare)), [plain("好"), plain("第二段")]);
    // Aura's own words are plain already.
    let wide = channel(100, false);
    assert_eq!(outgoing("**不转**", ReplyFormat::Text, Some(&wide)), [plain("**不转**")]);
    // A channel the service did not report: as it is, within what one protocol line holds.
    assert_eq!(outgoing(text, ReplyFormat::Markdown, None), [markdown(text)]);
    let long = "字".repeat(100_001);
    let pieces = outgoing(&long, ReplyFormat::Text, None);
    assert_eq!(
        pieces.iter().map(|(piece, _)| piece.chars().count()).collect::<Vec<_>>(),
        [100_000, 1]
    );
}

#[test]
fn a_code_block_cut_between_pieces_is_closed_and_opened_again() {
    let reply = "代码：\n\n```rust\nfn one() {}\nfn two() {}\nfn three() {}\n```\n\n完了。";
    let pieces = outgoing(reply, ReplyFormat::Markdown, Some(&channel(30, true)));
    let texts: Vec<&str> = pieces.iter().map(|(piece, _)| piece.as_str()).collect();
    assert_eq!(
        texts,
        [
            "代码：",
            "```rust\nfn one() {}\n```",
            "```rust\nfn two() {}\n```",
            "```rust\nfn three() {}\n```\n\n完了。",
        ]
    );
    assert!(pieces.iter().all(|(piece, format)| {
        piece.chars().count() <= 30 && *format == ReplyFormat::Markdown
    }));
    // Too little room to close a block and open it again: cut as plain text is.
    let narrow = outgoing(reply, ReplyFormat::Markdown, Some(&channel(12, true)));
    assert_eq!(
        narrow.into_iter().map(|(piece, _)| piece).collect::<Vec<_>>(),
        split_text(reply, 12)
    );
    // A fence longer than the room kept to close it: still within the limit.
    let long_fence = format!("``````````\n{}``````````", "x\n".repeat(30));
    let pieces = outgoing(&long_fence, ReplyFormat::Markdown, Some(&channel(30, true)));
    assert!(pieces.iter().all(|(piece, _)| piece.chars().count() <= 30), "{pieces:?}");
}
