import QtQuick
Item {
    id: root
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    Component.onCompleted: {
        t("E1", function() { return /\w/i.test("\u017f") + "," + /[a-z]/i.test("\u212a") + "," + /I/i.test("\u0131") })
        t("E2", function() { return /\u{1f600}/.test("u{1f600}") })
        t("E3", function() { return /[\ud83d\ude00]/.test("\ud83d") })
        t("E4", function() { return [/[\ud83d\ude00]/.test("\ude00"), /[\ud83d\ude00]/.test("\ud83d\ude00"), /^[\ud83d\ude00]$/.test("\ud83d\ude00"), /^[\ud83d\ude00]{2}$/.test("\ud83d\ude00"), /[😀]/.test("\ud83d"), /^[😀]$/.test("😀"), /^[😀]{2}$/.test("😀"), /[^😀]/.test("\ud83d"), /^.$/.test("😀"), /^..$/.test("😀"), /😀{2}/.test("😀😀"), /^\ud83d/.test("😀"), /\ude00$/.test("😀"), /[\ud83d-\ude00]/.test("\ud900")].join() })
        t("E5", function() { return [/\u{1f600}/u.test("😀"), /^[😀]$/u.test("😀"), /[😀]/u.test("\ud83d"), /^.$/u.test("😀"), /\u{61}/.test("a"), /\u{61}/u.test("a"), /a\u{2}/.test("auu"), /\u{/.test("u{"), /\u{}/.test("u{}")].join() })
        t("E6", function() { return [/[a-z]/i.test("\u017f"), /s/i.test("\u017f"), /\u017f/i.test("s"), /\u017f/i.test("S"), /k/i.test("\u212a"), /\u212a/i.test("k"), /[\u212a]/i.test("K"), /i/i.test("\u0130"), /\u0130/i.test("i"), /\u00df/i.test("SS"), /\u00e9/i.test("\u00c9"), /[\u00e0-\u00ff]/i.test("\u00c9"), /\u03c3/i.test("\u03c2"), /\u03a3/i.test("\u03c2"), /\u01c5/i.test("\u01c4"), /\u01c6/i.test("\u01c5"), /\u1e9e/i.test("\u00df"), /\W/i.test("\u017f"), /\W/i.test("s"), /[^\W]/i.test("\u212a"), /\b/i.test("\u017f"), /[\w-a]/i.test("\u212a")].join() })
        t("E7", function() { return [/s/iu.test("\u017f"), /\w/iu.test("\u017f"), /[a-z]/iu.test("\u212a"), /\W/iu.test("\u017f"), /\W/iu.test("S"), /k/iu.test("\u212a"), /\u00df/iu.test("\u1e9e")].join() })
        t("E8", function() { return ["\ud83d\ude00".replace(/[\ud83d]/, "x").length, "😀".split(/[\ude00]/).length, "😀".match(/[\ud800-\udbff]/) !== null, "😀".match(/[^\ud83d]/).index, "a😀".search(/[\ude00]/), JSON.stringify("😀b".match(/[\ud83d\ude00]+/)[0].length)].join() })
        t("E9", function() { return [new RegExp("[\ud83d\ude00]").test("\ud83d"), new RegExp("[\\ud83d\\ude00]").test("\ud83d"), new RegExp("\\u{1f600}").test("u{1f600}"), new RegExp("[😀]", "u").test("\ud83d"), /[\ud83d\ude00-\ude02]/.test("\ude01"), /[\ud83d\ude00-\ud83d\ude02]/u.test("😁")].join() })
        Qt.quit()
    }
}
