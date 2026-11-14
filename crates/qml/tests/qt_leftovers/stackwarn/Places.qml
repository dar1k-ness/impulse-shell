import QtQuick
import QtQuick.Controls

// Where and as what `qmlWarning(view)` names a StackView: the place is where the outermost
// document declares it, the type the name it has there.
Item {
    id: root

    component Foo: StackView {}
    component Bar: MyStack { property int more }

    StackView { id: plain }
    Item {
        StackView { id: nested; property int more }
    }
    MyStack { id: composite }
    MyStack { id: extended; property int more }
    Foo { id: inlineType }
    Bar { id: inlineOfComposite }
    Component { id: comp; StackView {} }
    Loader { id: loader; sourceComponent: comp }
    Loader { id: file; source: "MyStack.qml" }
    Repeater { id: rep; model: 2; StackView {} }

    Component.onCompleted: {
        const views = [
            plain, nested, composite, composite.inner, extended, inlineType, inlineOfComposite,
            loader.item, file.item, rep.itemAt(0), rep.itemAt(1),
            comp.createObject(root), comp.createObject(root),
            Qt.createQmlObject("import QtQuick.Controls\nStackView {}", root),
            Qt.createQmlObject("import QtQuick.Controls\n\n  StackView {}", root, "dyn/Name.qml"),
            Qt.createQmlObject("import QtQuick.Controls\nStackView {}", root, "dyn/name.qml"),
            Qt.createQmlObject("MyStack {}", root, "Other.qml"),
            Qt.createComponent("MyStack.qml").createObject(root),
            Qt.createComponent("lower.qml").createObject(root),
            Qt.createComponent("Deep.qml").createObject(root),
        ];
        for (const view of views)
            view.popToIndex(5);
        Qt.quit();
    }
}
