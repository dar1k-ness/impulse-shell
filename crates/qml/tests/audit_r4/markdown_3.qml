import QtQuick
Item {
    id: root
    property var cases: [
        "Sure! Here's how:\n\n## Steps\n\n1. Install `foo`:\n   ```bash\n   sudo pacman -S foo\n   ```\n2. Run it\n\n> **Note:** be careful\n\nThat's it!",
        "**Summary**\n\n- **A**: first thing\n- **B**: second thing with a [link](https://example.com/a_b)\n- *C*: third\n\n---\n\nFooter",
        "Line with `inline code` and **bold `code` inside** and ~~gone~~.",
        "| Name | Value |\n|------|-------|\n| **a** | `1` |\n| b | 2 |\n\nAfter table.",
        "1. one\n   continued\n2. two\n\n   para in two\n3. three",
        "- a\n\n  ```\n  code in list\n  ```\n- b",
        "> quote line\n> - item\n> - item2\n>\n> end quote",
        "Text\n\n    indented code\n\nText after",
        "### Heading\nParagraph directly\n\n#### Another\n- list",
        "Email <me@example.com> and https://example.com/path?q=1&r=2.",
        "x < y && y > z, a <= b, 5 * 3 = 15, snake_case_name, C:\\path\\file",
        "Use `--flag`, then *emphasis*, then __strong under__, then ***both***.",
        "- one\n- two\n  - two.a\n  - two.b\n    1. deep\n    2. deeper\n- three",
        "Final answer: **42**.\n\nSources:\n1. [A](http://a.b)\n2. [B](http://c.d)",
        "a\n\n\n\n\nb\n\n   \n\nc",
        "Tabs:\n\n\tcode with tab\n\nend",
        "![img](a.png)",
        "[only link](http://x.y)",
        "`only code`",
        "**only bold**",
        "# Only heading",
        "> only quote",
        "- only item",
        "1. only numbered",
        "```\nonly code block\n```",
        "term\n: not a def list",
        "foot[^1]\n\n[^1]: note",
        "H~2~O and x^2^ and ==mark==",
        "$math$ and $$block$$",
        "&copy; &nbsp; &#65; &unknown;",
        "trailing backslash\\",
        "a\\\nb",
        "* * *",
        "___",
        "- a\n- b\n\n\n- c",
        "1. a\n1. b\n1. c",
        "10. ten\n11. eleven",
        "100. hundred\n101. more",
        "word ".repeat(15) + "`code span` " + "word ".repeat(10),
        "word ".repeat(15) + "[link text](http://example.com/long/path) " + "word ".repeat(10),
        "- [ ] " + "word ".repeat(25),
        "> " + "word ".repeat(15) + "**bold** " + "word ".repeat(10)
    ]
    TextEdit { id: te; textFormat: TextEdit.MarkdownText }
    Component.onCompleted: {
        for (var i = 0; i < cases.length; i++) {
            te.textFormat = TextEdit.MarkdownText
            te.text = cases[i]
            te.textFormat = TextEdit.PlainText
            console.log(i, JSON.stringify(te.text))
        }
        console.log("END"); Qt.quit();
    }
}
