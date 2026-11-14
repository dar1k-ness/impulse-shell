import QtQuick
Item {
    id: root
    property var cases: [
        "",
        "a\n\n\n\nb",
        "  leading spaces\n   more",
        "1. one\n2. two\n3. three\n\nafter list",
        "3. three\n4. four",
        "1) paren\n2) two",
        "- a\n\n- b\n\n- c",
        "- a\n- b\n\npara after",
        "para before\n- a\n- b",
        "+ plus\n+ two",
        "- a\n  1. n1\n  2. n2\n- b",
        "1. a\n   - x\n   - y\n2. b",
        "- a\n  - b\n    - c\n- d",
        "- [ ] todo\n- [x] done",
        "- item with\n  continuation line",
        "- item\n\n  second para in item\n- next",
        "```js\nlet a = 1;\n\nlet b = 2;\n```",
        "text\n```\ncode\n```\nmore text",
        "~~~\ntilde fence\n~~~",
        "    indented code\n    more",
        "> q1\n> q2\n\n> q3",
        "> nested\n>> deeper",
        "> - list in quote\n> - two",
        "---",
        "above\n\n***\n\nbelow",
        "Setext H1\n=========\n\nSetext H2\n---------",
        "# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6",
        "# Heading with *emph* and `code`",
        "***bold italic*** and **bold *nested italic* bold**",
        "__under strong__ and _under_ and *it*",
        "a ``code with ` tick`` b",
        "`a` `b`",
        "[text](http://x.y \"title\") and <http://auto.link> and http://bare.link",
        "![alt](img.png) and ![](x.png \"t\")",
        "esc \\* star \\_ under \\# hash 1\\. num",
        "* not list?\n*text",
        "2 * 3 * 4 and a_b_c and <tag and a & b and a &amp; b &lt;",
        "line one\\\nbackslash break",
        "| l | c | r |\n|:--|:-:|--:|\n| longer cell | x | y |\n| a | bb | ccc |",
        "before\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nafter",
        "<b>bold</b> <i>it</i> <u>un</u> <s>st</s> <code>c</code> <br> x",
        "<div>block html</div>\n\npara",
        "word ".repeat(20) + "**bold " + "word ".repeat(10) + "end**",
        "- " + "word ".repeat(30),
        "1. " + "word ".repeat(30),
        "> " + "word ".repeat(30),
        "x".repeat(100),
        "abc ".repeat(10) + "x".repeat(100) + " tail",
        "# " + "word ".repeat(30),
        "Here is a list:\n\n1. **First**: do this\n2. **Second**: do that\n   - sub a\n   - sub b\n\nThen `run` it:\n\n```bash\nmake all\n```\n\nDone.",
        "tab\tinside and  double  spaces",
        "ends with two spaces  ",
        "a\n  \nb",
        "- a\n- b\n\n1. x\n2. y",
        "- a\n\n\n- b",
        "1. a\n\n2. b",
        "**bold**text and *it*text",
        "#not heading\n#5 issue",
        "-not list\n1.not list",
        "\\[x\\] and [ref] and [a](b) c",
        "a  \nb  \nc",
        "> quote\n\npara\n\n> quote2",
        "- a\n# h\n- b",
        "text\n# h\ntext2\n- l\ntext3",
        "😀 emoji and ünïcödé " + "слово ".repeat(20)
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
