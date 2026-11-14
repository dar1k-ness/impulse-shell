import QtQuick
import QtQuick.Controls
import QtQml.Models
import Qt.labs.folderlistmodel
import Qt.labs.qmlmodels

Item {
    id: root
    width: 200
    height: 200

    function show(v) {
        if (v === undefined)
            return "undefined";
        if (v === null)
            return "null";
        if (typeof v === "object" && v.objectName !== undefined)
            return "the object " + v.objectName;
        if (Array.isArray(v))
            return "array of " + v.length;
        return typeof v + " " + v;
    }
    function probe(label, o, name, call) {
        let line = label + ": typeof " + typeof o[name];
        try {
            line += ", returns " + show(call(o));
        } catch (e) {
            line += ", throws " + e;
        }
        console.log(line);
    }

    ComboBox {
        id: combo
        model: ["Apple", "banana", "Cherry"]
    }
    ComboBox {
        id: editable
        editable: true
        model: ["one", "two"]
        contentItem: TextInput {
            text: "one"
        }
    }
    Menu {
        id: menu
        MenuItem { text: "a" }
    }
    Canvas {
        id: canvas
        width: 10
        height: 10
    }
    DelegateChooser {
        id: chooser
        role: "kind"
        DelegateChoice { roleValue: "a"; Item { objectName: "made" } }
    }
    FolderListModel {
        id: folder
    }

    Component.onCompleted: {
        probe("ComboBox.find exact", combo, "find", o => o.find("banana"));
        probe("ComboBox.find case", combo, "find", o => o.find("BANANA"));
        probe("ComboBox.find none", combo, "find", o => o.find("nope"));
        probe("ComboBox.find starts", combo, "find", o => o.find("Ch", Qt.MatchStartsWith));
        probe("ComboBox.find contains", combo, "find", o => o.find("err", Qt.MatchContains));
        probe("ComboBox.find sensitive", combo, "find", o => o.find("apple", Qt.MatchFixedString | Qt.MatchCaseSensitive));
        probe("ComboBox.find no args", combo, "find", o => o.find());
        probe("ComboBox.selectAll", combo, "selectAll", o => o.selectAll());
        probe("ComboBox.selectAll editable", editable, "selectAll", o => {
            const r = o.selectAll();
            return r + " selected '" + o.contentItem.selectedText + "'";
        });
        probe("Menu.dismiss", menu, "dismiss", o => o.dismiss());
        probe("Menu.dismiss open", menu, "dismiss", o => {
            o.open();
            const r = o.dismiss();
            return r + " visible " + o.visible;
        });
        probe("Canvas.save no args", canvas, "save", o => o.save());
        probe("Canvas.save empty", canvas, "save", o => o.save(""));
        probe("DelegateChooser.createObject", chooser, "createObject", o => o.createObject(root));
        probe("DelegateChooser.createObject no args", chooser, "createObject", o => o.createObject());
        probe("FolderListModel.sort", folder, "sort", o => o.sort(0));
        probe("FolderListModel.sort no args", folder, "sort", o => o.sort());
        probe("FolderListModel.submit", folder, "submit", o => o.submit());
        probe("FolderListModel.match no args", folder, "match", o => o.match());
        probe("FolderListModel.match null", folder, "match", o => o.match(null, 0, "x").length);
        probe("FolderListModel.match undefined", folder, "match", o => o.match(undefined, 0, "x").length);
        probe("FolderListModel.match number", folder, "match", o => o.match(0, 0, "x").length);
        probe("FolderListModel.match two args", folder, "match", o => o.match(null, 0));
        probe("ComboBox.find number", combo, "find", o => o.find(1));
        probe("ComboBox.find regexp", combo, "find", o => o.find("^b.*a$", Qt.MatchRegularExpression));
        probe("ComboBox.find wildcard", combo, "find", o => o.find("*err?", Qt.MatchWildcard));
        probe("ComboBox.find ends", combo, "find", o => o.find("RY", Qt.MatchEndsWith));
        probe("ComboBox.find ends sensitive", combo, "find", o => o.find("RY", Qt.MatchEndsWith | Qt.MatchCaseSensitive));
        probe("ComboBox.find fixed", combo, "find", o => o.find("BANANA", Qt.MatchFixedString));
        probe("Canvas.save size", canvas, "save", o => o.save("", Qt.size(4, 4)));
        probe("Qt.destroy", Qt, "destroy", o => o.destroy());
        Qt.quit();
    }
}
