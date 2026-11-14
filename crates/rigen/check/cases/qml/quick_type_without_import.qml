// `Component` is a type of QtQml, which a file gets through `import QtQuick`; this file imports
// neither. Qt does not load it: "Non-existent attached object" at the handler. We load it and
// run the handler: a difference of the engine, not fixed.
// qt: load error 7:5: Non-existent attached object
// ours: type 6
DirectoryImportAliasType {
    Component.onCompleted: n = 6
}
