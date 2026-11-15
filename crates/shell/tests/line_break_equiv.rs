//! Where a line may be broken, compared with Qt itself (`qml6`, offscreen) on every string of
//! the Unicode line breaking conformance test (`line_break/LineBreakTest.txt`, Unicode 17, its
//! comments left out). Qt does not follow the test's own answers everywhere (its rules are
//! those of `QUnicodeTools`, with Thai broken by libthai), so what counts is Qt's: each
//! string is wrapped at a width of one pixel, where every opportunity is taken, and compared
//! are the number of lines of a Text and the character every line starts at. Qt tells the
//! starts through a TextInput, which lays its text out as a Text does and shows where a
//! character is; a string a TextInput changes or lays out in other lines than a Text (it has
//! a line separator) is compared by its number of lines alone.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const PROPS: &str = "width: 1; font.family: \"Noto Sans\"; font.pixelSize: 16";

fn strings() -> Vec<String> {
    include_str!("line_break/LineBreakTest.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            l.split_whitespace()
                .filter(|part| !matches!(*part, "×" | "÷"))
                .map(|cp| char::from_u32(u32::from_str_radix(cp, 16).unwrap()).unwrap())
                .collect()
        })
        .collect()
}

/// Thai, which has no spaces between words: Qt asks libthai's dictionary where they end.
/// Sentences, Thai next to Latin, digits and punctuation, a word no dictionary has, and
/// for comparison Lao, Khmer and Myanmar, which Qt has no dictionary for.
const THAI: [&str; 12] = [
    "ภาษาไทยไม่มีช่องว่างระหว่างคำ",
    "สวัสดีครับ ยินดีต้อนรับสู่ประเทศไทย",
    "กรุงเทพมหานครเป็นเมืองหลวงของประเทศไทย",
    "ราคา 1,250 บาท (รวมภาษีมูลค่าเพิ่ม 7%)",
    "เปิดไฟล์ README.md แล้วกดปุ่ม Enter",
    "Wi-Fiเชื่อมต่อแล้วที่ความเร็ว100Mbps",
    "ผู้ใช้ “ทดสอบ” ออกจากระบบแล้ว…",
    "น้ำำำก็่่่ที่นี่",
    "กขฃคฅฆงจฉชซฌญฎฏ",
    "ພາສາລາວບໍ່ມີຊ່ອງຫວ່າງ",
    "ភាសាខ្មែរមិនមានចន្លោះ",
    "မြန်မာဘာသာစကား",
];

/// (`lineCount`, the UTF-16 offset every line after the first starts at, when known)
type Lines = (usize, Option<Vec<usize>>);

fn qt_lines(strings: &[String], wrap: &str) -> Vec<Lines> {
    let mut s = format!(
        "import QtQuick\nItem {{\n    Text {{ id: t; {PROPS}; wrapMode: Text.{wrap}; \
         textFormat: Text.PlainText }}\n    TextInput {{ id: e; {PROPS}; \
         wrapMode: TextInput.{wrap} }}\n    property var cases: [\n"
    );
    for string in strings {
        s.push('"');
        for unit in string.encode_utf16() {
            s += &format!("\\u{unit:04X}");
        }
        s += "\",\n";
    }
    s += "    ]
    Component.onCompleted: {
        for (let k = 0; k < cases.length; k++) {
            t.text = cases[k];
            e.text = cases[k];
            let starts = [], y = e.positionToRectangle(0).y;
            for (let p = 1; p < e.length; p++) {
                const r = e.positionToRectangle(p);
                if (r.y !== y) {
                    starts.push(p);
                    y = r.y;
                }
            }
            const same = e.text === cases[k] && starts.length + 1 === t.lineCount;
            console.log(\"CASE\", t.lineCount, same ? \"=\" + starts.join(\",\") : \"\");
        }
        Qt.quit();
    }
}
";
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("breaks.qml");
    std::fs::write(&file, s).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<Lines> = log
        .lines()
        .filter_map(|l| l.split_once("CASE ")?.1.split_once(' '))
        .map(|(count, starts)| {
            let starts = starts.strip_prefix('=').map(|starts| {
                starts
                    .split(',')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.parse().unwrap())
                    .collect()
            });
            (count.parse().unwrap(), starts)
        })
        .collect();
    assert_eq!(lines.len(), strings.len(), "qml6 printed:\n{log}");
    lines
}

fn our_lines(strings: &[String], wrap: &str) -> Vec<Vec<usize>> {
    let mut rt = Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt
        .load_str(&format!(
            "import QtQuick\nItem {{\n    property Item item: t\n    Text {{ id: t; {PROPS}; \
             wrapMode: Text.{wrap}; textFormat: Text.PlainText }}\n}}\n"
        ))
        .unwrap();
    rt.settle();
    let mut w = rt.world.borrow_mut();
    let Some(Value::Item(t)) = w.read(root, "item") else {
        panic!("no Text");
    };
    strings
        .iter()
        .map(|string| {
            w.write(t, "text", Value::Str(string.as_str().into()));
            scene::text_line_starts(&mut w, &mut fonts, t)
                .iter()
                .map(|&start| string[..start].encode_utf16().count())
                .collect()
        })
        .collect()
}

/// What differs between Qt and us on `strings`, and how many of them Qt tells line starts for.
fn differences(strings: &[String]) -> (Vec<String>, usize) {
    differences_wrapped(strings, "WordWrap")
}

/// [`differences`] for the `wrapMode` `wrap`.
fn differences_wrapped(strings: &[String], wrap: &str) -> (Vec<String>, usize) {
    let (qt, ours) = (qt_lines(strings, wrap), our_lines(strings, wrap));
    let located = qt.iter().filter(|(_, starts)| starts.is_some()).count();
    let wrong: Vec<String> = strings
        .iter()
        .zip(qt.iter().zip(&ours))
        .filter(|(_, ((count, starts), ours))| {
            *count != ours.len() || starts.as_ref().is_some_and(|starts| *starts != ours[1..])
        })
        .map(|(string, (qt, ours))| {
            let string: Vec<String> = string
                .chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect();
            format!("{}\n    ours {ours:?}\n    Qt   {qt:?}", string.join(" "))
        })
        .collect();
    (wrong, located)
}

#[test]
fn lines_break_where_qt_breaks_them() {
    if !testenv::require("qml6") {
        return;
    }
    let strings = strings();
    let (wrong, located) = differences(&strings);
    assert!(
        wrong.is_empty(),
        "{} of {} strings break differently in Qt (lineCount, and for {located} of them line \
         starts):\n{}",
        wrong.len(),
        strings.len(),
        wrong.join("\n")
    );
}

/// Scripts whose syllables are several characters, for `Text.WrapAnywhere`, which breaks
/// between any two of what Qt takes for one character (QCharAttributes::graphemeBoundary):
/// consonant clusters joined by a virama, with a nukta, a reph, ZWJ and ZWNJ, vowel signs
/// of two parts, a virama at the end of a word; Tibetan stacks, tsheg and shad; Khmer coeng
/// clusters and vowels around a consonant; digits, Latin and spaces beside them.
const SYLLABIC: [&str; 40] = [
    "हिन्दी भाषा में स्वागत है",
    "क्षत्रिय ज्ञान श्रद्धा द्वार",
    "कर्म धर्म आर्य र्‍य",
    "क़िला ज़रा फ़ोन ड़ ढ़",
    "क्‍ष क्‌ष रू रु द्ध्र्य",
    "संस्कृतम् अँगूठा दुःख ॐ",
    "राष्ट्रपति विद्यार्थी स्त्री 123 abc",
    "বাংলা ভাষায় স্বাগতম",
    "ক্ষত্রিয় জ্ঞান শ্রদ্ধা কর্ম",
    "কো কৌ কে কৈ র‍্য ৎ",
    "ਪੰਜਾਬੀ ਭਾਸ਼ਾ ਵਿੱਚ ਸੁਆਗਤ",
    "ਪ੍ਰੇਮ ਸ਼੍ਰੀ ਖ਼ਾਲਸਾ ਅੱਜ",
    "ગુજરાતી ભાષામાં સ્વાગત છે",
    "ક્ષત્રિય જ્ઞાન શ્રદ્ધા કર્મ",
    "ଓଡ଼ିଆ ଭାଷାରେ ସ୍ୱାଗତ",
    "କ୍ଷତ୍ରିୟ ଜ୍ଞାନ କୋ କୌ କୈ",
    "தமிழ் மொழியில் வரவேற்கிறோம்",
    "கொ கோ கௌ ஸ்ரீ க்ஷ",
    "தெலுங்கு க் ங் ச் ஞ்",
    "తెలుగు భాషలో స్వాగతం",
    "క్షత్రియ జ్ఞానం శ్రద్ధ కై",
    "ಕನ್ನಡ ಭಾಷೆಯಲ್ಲಿ ಸ್ವಾಗತ",
    "ಕ್ಷತ್ರಿಯ ಜ್ಞಾನ ಕೊ ಕೋ ಕೈ ರ್‍ಯ",
    "മലയാളം ഭാഷയിൽ സ്വാഗതം",
    "ക്ഷത്രിയ ജ്ഞാനം കൊ കോ കൌ",
    "അവൻ അവൾ അവർ ന്‍ ര്‍ ല്‍",
    "සිංහල භාෂාවෙන් සාදරයෙන්",
    "ශ්‍රී ක්‍ෂ කො කෝ කෞ ද්‍ය",
    "བོད་སྐད་ལ་དགའ་བསུ་ཞུ།",
    "རྒྱལ་པོ་སྒྲུབ་བརྒྱུད་ཧཱུྃ༔",
    "ཨོཾ་མ་ཎི་པདྨེ་ཧཱུྃ། ༡༢༣",
    "བཀྲ་ཤིས་བདེ་ལེགས། abc",
    "ភាសាខ្មែរ សូមស្វាគមន៍",
    "ស្ត្រី ក្រុង ព្រះ ខ្ញុំ ស្រុក",
    "កោ កើ កឿ កៀ កែ កៃ កៅ",
    "កម្ពុជា រាជធានីភ្នំពេញ ១២៣",
    "សំរាប់ ទាំង ចំនួន អ៊ំ ប៉ុន្តែ",
    "ক্‌ষ ক্‍ষ र्‌क ക്‍ ක්‍ය",
    "aक्षb ক্ষ1 ស្ត2 རྒྱ3",
    "क् ক্ ક્ క్ ക് ් ្ ྄",
];

/// How many times every string of `SYLLABIC` is shuffled.
const SHUFFLES: usize = 8;

/// `Text.WrapAnywhere` breaks a text of an Indic script, of Tibetan and of Khmer between the
/// same characters as Qt, whose rules for these scripts move the boundaries of a character
/// (`indicAttributes`, `tibetanAttributes`, `khmerAttributes` of QUnicodeTools).
#[test]
fn wrap_anywhere_breaks_syllabic_scripts_where_qt_does() {
    if !testenv::require("qml6") {
        return;
    }
    let mut strings: Vec<String> = SYLLABIC.iter().map(|s| s.to_string()).collect();
    // The same characters in orders no word has them in, ZWJ and ZWNJ among them: marks
    // with nothing to stand on, two viramas, a vowel sign after a vowel sign.
    let mut seed = 17u32;
    let mut random = |below: usize| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as usize % below
    };
    for string in SYLLABIC {
        for _ in 0..SHUFFLES {
            let mut chars: Vec<char> = string.chars().chain("\u{200c}\u{200d}".chars()).collect();
            for k in (1..chars.len()).rev() {
                chars.swap(k, random(k + 1));
            }
            strings.push(chars.into_iter().collect());
        }
    }
    let (wrong, located) = differences_wrapped(&strings, "WrapAnywhere");
    elog::eprintln!(
        "{} strings wrapped anywhere compared with Qt (line starts of {located}), {} differ",
        strings.len(),
        wrong.len()
    );
    assert!(
        wrong.is_empty(),
        "{} of {} strings break differently in Qt (lineCount, and for {located} of them line \
         starts):\n{}",
        wrong.len(),
        strings.len(),
        wrong.join("\n")
    );
}

/// The number of lines of every Thai string at width 1, in Qt and ours.
fn thai_line_counts(strings: &[String]) -> String {
    let qt: Vec<usize> = qt_lines(strings, "WordWrap").iter().map(|l| l.0).collect();
    let ours: Vec<usize> = our_lines(strings, "WordWrap")
        .iter()
        .map(Vec::len)
        .collect();
    format!("lines in Qt {qt:?}, ours {ours:?}")
}

/// What `impulse-shell --version` says about Thai on this machine (tools/install.sh reads it).
fn version_says_of_thai() -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
        .arg("--version")
        .output()
        .unwrap();
    let out = String::from_utf8(out.stdout).unwrap();
    let line = out
        .lines()
        .find_map(|l| l.strip_prefix("Thai line breaking: "));
    line.unwrap_or_else(|| panic!("--version says nothing of Thai:\n{out}"))
        .to_string()
}

/// Thai breaks between the words libthai finds, as in Qt: the image has the library, so a
/// sentence without a space is more than one line, and `impulse-shell --version` says so.
#[test]
fn thai_breaks_between_words() {
    if !testenv::require("qml6") {
        return;
    }
    let strings: Vec<String> = THAI.iter().map(|s| s.to_string()).collect();
    let words = our_lines(&strings[..1], "WordWrap")[0].len();
    assert!(words > 3, "a Thai sentence is {words} lines");
    assert_eq!(version_says_of_thai(), "between words (libthai)");
    let (wrong, located) = differences(&strings);
    assert!(
        wrong.is_empty(),
        "{} of {} strings break differently in Qt (lineCount, and for {located} of them line \
         starts):\n{}",
        wrong.len(),
        strings.len(),
        wrong.join("\n")
    );
    elog::eprintln!(
        "{} strings compared with Qt with libthai, 0 differ: {}",
        strings.len(),
        thai_line_counts(&strings)
    );
}

/// The file `libthai.so.0` is on this machine, as the dynamic loader finds it.
fn libthai_file() -> Option<std::path::PathBuf> {
    // SAFETY: loading libthai runs no initialisers with preconditions.
    let _lib = unsafe { libloading::Library::new("libthai.so.0") }.ok()?;
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    let file = maps
        .lines()
        .filter_map(|l| l.split_whitespace().nth(5))
        .find(|file| file.contains("/libthai.so"))?;
    Some(file.into())
}

const WITHOUT_LIBTHAI: &str = "thai_without_libthai_child";

/// On a machine without libthai Thai breaks at spaces only, as in Qt, which opens the library
/// the same way (`QLibrary("thai", 0)`): the test runs again where the library's file cannot
/// be loaded (a mount namespace with an empty file in its place), Qt in it too.
#[test]
fn thai_without_libthai_breaks_at_spaces_only() {
    if !testenv::require_all(&["qml6", "unshare", "mount"]) {
        return;
    }
    let Some(file) = libthai_file() else {
        testenv::unavailable("libthai (`libthai.so.0`) is not installed");
        return;
    };
    let out = std::process::Command::new("unshare")
        .args(["--user", "--map-root-user", "--mount", "sh", "-c"])
        .arg("mount --bind /dev/null \"$1\" || exit 77; shift; exec \"$@\"")
        .arg("sh")
        .arg(&file)
        .arg(std::env::current_exe().unwrap())
        .args([WITHOUT_LIBTHAI, "--exact", "--ignored", "--nocapture"])
        .output()
        .unwrap();
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.code() == Some(77) || log.contains("unshare failed") {
        testenv::unavailable(format_args!(
            "unprivileged mount namespaces (`unshare --user --map-root-user --mount`): {log}"
        ));
        return;
    }
    assert!(
        out.status.success() && log.contains(&format!("test {WITHOUT_LIBTHAI} ... ok")),
        "without {}:\n{log}",
        file.display()
    );
    elog::eprintln!(
        "{}",
        log.lines()
            .find(|l| l.contains("compared with Qt"))
            .unwrap_or("")
    );
}

#[test]
#[ignore = "child process: thai_without_libthai_breaks_at_spaces_only runs it where libthai cannot be loaded"]
fn thai_without_libthai_child() {
    assert!(libthai_file().is_none(), "libthai can be loaded here");
    let strings: Vec<String> = THAI.iter().map(|s| s.to_string()).collect();
    // The sentences without a space are one line each, the others break at their spaces.
    let ours = our_lines(&strings[..3], "WordWrap");
    let lines: Vec<usize> = ours.iter().map(Vec::len).collect();
    assert_eq!(lines, [1, 2, 1], "{ours:?}");
    assert_eq!(
        version_says_of_thai(),
        "at spaces only (libthai.so.0 is not installed)"
    );
    let (wrong, located) = differences(&strings);
    assert!(
        wrong.is_empty(),
        "{} of {} strings break differently in a Qt without libthai (lineCount, and for \
         {located} of them line starts):\n{}",
        wrong.len(),
        strings.len(),
        wrong.join("\n")
    );
    elog::eprintln!(
        "{} strings compared with Qt without libthai, 0 differ: {}",
        strings.len(),
        thai_line_counts(&strings)
    );
}
