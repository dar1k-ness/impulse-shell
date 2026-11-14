import QtQuick
Item {
    id: root
    property var cases: [
        "plain text",
        "# Title\n\nsome *italic* and **bold** text",
        "* one\n* two\n* three",
        "- a\n- b",
        "1. first\n2. second",
        "line one\nline two\n\nnext para",
        "a `code` span and [link](http://x.y)",
        "```\ncode block\n  indented\n```",
        "> quote here",
        "word ".repeat(30),
        "_under_ and ~~strike~~",
        "| a | b |\n|---|---|\n| 1 | 2 |",
        "text with <b>html</b> & amp",
        "## H2\ntext right after",
        "- a\n  - nested\n- b",
        "trailing spaces  \nhard break"
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
