// The qualifier of `import "dir" as Here` is an object in JavaScript, and `Here.Type` is the
// type `Type` names: its enums are read through it.
// qt: object object 1 2 5 true 0
import QtQuick
import "." as Here
Item {
    id: root
    property var made: Here.DirectoryImportAliasType { }
    property string result: [typeof Here, typeof Here.DirectoryImportAliasType, Here.DirectoryImportAliasType.B,
        Here.DirectoryImportAliasType.Mode.C, made.n, "DirectoryImportAliasType" in Here, Object.keys(Here).length].join(" ")
}
