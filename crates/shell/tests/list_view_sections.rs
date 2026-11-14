//! ListView `section` with InlineLabels (waffle start menu search results): a header before
//! each run of equal `section.property` values, laid out like QQuickListView (measured in qs:
//! headers at 0 and 88, delegates at 20, 54 and 108, contentHeight 138).

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r##"
import QtQuick
import Rustshell
FloatingWindow {
    width: 300; height: 300
    property var view: lv
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 4
        model: [{ n: "a", c: "X" }, { n: "b", c: "X" }, { n: "c", c: "Y" }]
        section {
            property: "c"
            criteria: ViewSection.FullString
            labelPositioning: ViewSection.InlineLabels
            delegate: Rectangle { required property string section; width: ListView.view?.width ?? 0; height: 20; objectName: "sec:" + section }
        }
        delegate: Rectangle { required property var modelData; width: 300; height: 30; objectName: "del:" + modelData.n }
    }
}
"##;

#[test]
fn inline_section_headers_are_laid_out_before_their_runs() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(QML).unwrap();
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [300.0, 300.0]);
    }
    let Some(Value::Item(lv)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let mut got: Vec<(String, f64)> = Vec::new();
    let rects = rt.world.borrow_mut().of_type("Rectangle");
    for r in rects {
        let name = rt.get(r, "objectName").to_js_string();
        if !name.is_empty() {
            got.push((name, rt.get(r, "y").to_number()));
        }
    }
    got.sort_by(|a, b| a.1.total_cmp(&b.1));
    let want: Vec<(String, f64)> = [
        ("sec:X", 0.0),
        ("del:a", 20.0),
        ("del:b", 54.0),
        ("sec:Y", 88.0),
        ("del:c", 108.0),
    ]
    .iter()
    .map(|(n, y)| (n.to_string(), *y))
    .collect();
    assert_eq!(got, want);
    assert_eq!(rt.get(lv, "contentHeight").to_number(), 138.0);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// The audit's `flick/sec` views; each expected string is what Qt 6.12 printed for them.
const SECTIONS: &str = r##"
import QtQuick
import Rustshell
FloatingWindow {
    id: root
    width: 200; height: 400
    property string log: ""
    property string order: ""
    property var rows: [ {name: "a", category: "Apps"}, {name: "b", category: "Apps"}, {name: "c", category: "Files"}, {name: "d", category: "apps"}, {name: "e", category: "Apps"}, {name: "f", category: ""}, {name: "g", category: "Web"}, {name: "h", category: "Web"} ]
    ListView {
        id: lv
        width: 200; height: 400; spacing: 4
        model: root.rows
        section { property: "category"
            delegate: Item { required property string section; width: 200; height: 42; objectName: "head:" + section
                Component.onCompleted: root.order += "head " + section + "|" }
        }
        delegate: Rectangle { required property var modelData; required property int index; width: 200; height: 20
            function dump() { return index + ":" + ListView.previousSection + "|" + ListView.section + "|" + ListView.nextSection }
            Component.onCompleted: { root.log += "made " + index + " " + JSON.stringify(ListView.section) + "\n"; root.order += "row " + index + "|" }
            ListView.onSectionChanged: root.log += "section " + index + " " + ListView.section + "\n"
            ListView.onPreviousSectionChanged: root.log += "prev " + index + " " + ListView.previousSection + "\n"
            ListView.onNextSectionChanged: root.log += "next " + index + " " + ListView.nextSection + "\n"
        }
        onCurrentSectionChanged: root.log += "current " + currentSection + "\n"
    }
    ListModel { id: lm
        ListElement { name: "apple"; kind: "fruit" } ListElement { name: "avocado"; kind: "fruit" } ListElement { name: "beet"; kind: "veg" } ListElement { name: "banana"; kind: "fruit" } ListElement { name: "Bean"; kind: "veg" } }
    ListView {
        id: lv2
        width: 200; height: 400
        model: lm
        section.property: "name"; section.criteria: ViewSection.FirstCharacter
        section.delegate: Rectangle { required property string section; width: 200; height: 15; objectName: "head2:" + section }
        delegate: Rectangle { required property string name; required property int index; width: 200; height: 20
            function dump() { return index + ":" + ListView.previousSection + "|" + ListView.section + "|" + ListView.nextSection } }
    }
    ListView {
        id: lv3
        width: 200; height: 400
        model: lm
        section.property: "kind"
        delegate: Rectangle { required property string name; width: 200; height: 20 }
    }
    ListView {
        id: lv4
        width: 200; height: 100
        model: 40
        section.property: "modelData"; section.criteria: ViewSection.FirstCharacter
        delegate: Rectangle { required property int index; width: 200; height: 20
            function dump() { return index + ":" + ListView.previousSection + "|" + ListView.section + "|" + ListView.nextSection } }
    }
    function all(v, n) { var s = []; for (var i = 0; i < n; i++) s.push(v.itemAtIndex(i).dump()); return s.join(" ") }
    function rowsOf(which) { return which == 1 ? all(lv, 8) : which == 2 ? all(lv2, 5) : all(lv4, 6) }
    function current(which) { return which == 1 ? lv.currentSection : which == 2 ? lv2.currentSection : which == 3 ? lv3.currentSection : lv4.currentSection }
    function scroll(which, y) { (which == 1 ? lv : lv4).contentY = y; return current(which) }
    function heights() { return lv.contentHeight + " " + lv2.contentHeight }
    function made() { var s = []; for (var i = 0; i < lv4.count; i++) { var it = lv4.itemAtIndex(i); if (it) s.push(it.dump()) } return s.length + " " + s[s.length - 1] }
}
"##;

struct Sections {
    rt: Runtime,
    win: usize,
}

impl Sections {
    fn new() -> Sections {
        let mut rt = Runtime::new(None);
        let win = rt.load_str(SECTIONS).unwrap();
        let mut fonts = scene::Fonts::default();
        for _ in 0..3 {
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts, win, [200.0, 400.0]);
        }
        Sections { rt, win }
    }

    fn call(&mut self, f: &str, args: &[f64]) -> String {
        let args = args.iter().map(|&a| Value::Num(a)).collect();
        let r = self.rt.call_method(self.win, f, args).unwrap().unwrap();
        assert!(self.rt.errors().is_empty(), "{:?}", self.rt.errors());
        r.to_js_string()
    }

    /// The `section` of each header named `prefix`..., from the top of its view down.
    fn heads(&mut self, ty: &str, prefix: &str) -> Vec<String> {
        let items = self.rt.world.borrow_mut().of_type(ty);
        let mut got = Vec::new();
        for i in items {
            let name = self.rt.get(i, "objectName").to_js_string();
            if let Some(s) = name.strip_prefix(prefix) {
                got.push((self.rt.get(i, "y").to_number(), s.to_string()));
            }
        }
        got.sort_by(|a, b| a.0.total_cmp(&b.0));
        got.into_iter().map(|g| g.1).collect()
    }
}

#[test]
fn sections_are_compared_without_case() {
    let mut s = Sections::new();
    // "apps" after "Files" starts a section, "Apps" after "apps" does not.
    assert_eq!(
        s.heads("Item", "head:"),
        ["Apps", "Files", "apps", "", "Web"]
    );
    assert_eq!(s.call("heights", &[]), "398 130");
}

#[test]
fn first_character_sections_keep_their_case() {
    let mut s = Sections::new();
    assert_eq!(s.heads("Rectangle", "head2:"), ["a", "b"]);
    assert_eq!(
        s.call("rowsOf", &[2.0]),
        "0:|a|a 1:a|a|b 2:a|b|b 3:b|b|B 4:b|B|"
    );
}

#[test]
fn delegates_know_their_section_and_the_ones_around() {
    let mut s = Sections::new();
    assert_eq!(
        s.call("rowsOf", &[1.0]),
        "0:|Apps|Apps 1:Apps|Apps|Files 2:Apps|Files|apps 3:Files|apps|Apps 4:apps|Apps| \
         5:Apps||Web 6:|Web|Web 7:Web|Web|"
    );
    // The row after the last one the view holds counts too.
    assert_eq!(
        s.call("rowsOf", &[4.0]),
        "0:|0|1 1:0|1|2 2:1|2|3 3:2|3|4 4:3|4|5 5:4|5|6"
    );
    // They are empty while the delegate completes; each then changes once.
    let log = s.rt.get(s.win, "log").to_js_string();
    let made: Vec<&str> = log.lines().filter(|l| l.starts_with("made")).collect();
    assert_eq!(made.len(), 8);
    assert!(made.iter().all(|l| l.ends_with("\"\"")), "{made:?}");
    let changes: Vec<&str> = log.lines().filter(|l| !l.starts_with("made")).collect();
    assert_eq!(
        changes,
        [
            "section 0 Apps",
            "next 0 Apps",
            "prev 1 Apps",
            "section 1 Apps",
            "next 1 Files",
            "prev 2 Apps",
            "section 2 Files",
            "next 2 apps",
            "prev 3 Files",
            "section 3 apps",
            "next 3 Apps",
            "prev 4 apps",
            "section 4 Apps",
            "prev 5 Apps",
            "next 5 Web",
            "section 6 Web",
            "next 6 Web",
            "prev 7 Web",
            "section 7 Web",
            "current Apps",
        ]
    );
}

#[test]
fn current_section_is_the_first_row_in_sight() {
    let mut s = Sections::new();
    assert_eq!(s.call("current", &[1.0]), "Apps");
    assert_eq!(s.call("current", &[2.0]), "a");
    // A view needs no section.delegate to have sections.
    assert_eq!(s.call("current", &[3.0]), "fruit");
    assert_eq!(s.call("current", &[4.0]), "0");
    // It changes inside the assignment that scrolls the view.
    assert_eq!(s.call("scroll", &[1.0, 100.0]), "Files");
    assert_eq!(s.call("scroll", &[1.0, 62.0]), "Apps");
    assert_eq!(s.call("scroll", &[1.0, 61.0]), "Apps");
    // Row "f" has an empty section.
    assert_eq!(s.call("scroll", &[1.0, 300.0]), "");
    assert_eq!(s.call("scroll", &[4.0, 400.0]), "2");
}

#[test]
fn a_section_header_is_created_right_after_its_first_row() {
    let s = Sections::new();
    assert_eq!(
        s.rt.get(s.win, "order").to_js_string(),
        "row 0|head Apps|row 1|row 2|head Files|row 3|head apps|row 4|row 5|head |row 6|\
         head Web|row 7|"
    );
}

#[test]
fn a_view_creates_the_rows_that_start_inside_its_cache_buffer() {
    let mut s = Sections::new();
    // 100 high, 20 a row, cacheBuffer 320: rows 0..=21 start at or before 420. The row after
    // the last one made still names the last one's nextSection.
    assert_eq!(s.call("made", &[]), "22 21:2|2|2");
}
