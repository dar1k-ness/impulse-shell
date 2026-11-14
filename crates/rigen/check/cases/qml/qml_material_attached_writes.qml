// The attached object of `Material` has the members of QQuickMaterialStyle and no others (Qt
// 6.12): its properties take a write, a new name and an Item's are dropped (strict code gets
// `TypeError: Type error`), a read-only property, a signal and its handler say so, an enum
// value stays.
// qt: accent #102030;accent write #405060;primary write #010203;containerStyle write true;theme write true;roundedScale write true;new undefined;new strict TypeError: Type error;new [k] undefined;item member undefined;read-only TypeError: Cannot assign to read-only property "primaryColor";signal TypeError: Cannot assign to read-only property "accentChanged";handler TypeError: Cannot assign to read-only property "accentChanged";enum 1;enum strict TypeError: Type error;type enum 1;
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
Item {
    id: root
    property string result: ""
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    TextField {
        id: f
        Material.accent: "#102030"
        Material.containerStyle: Material.Outlined
    }
    Component.onCompleted: run()
    function run() {
        t("accent", () => f.Material.accent)
        t("accent write", () => { f.Material.accent = "#405060"; return f.Material.accent })
        t("primary write", () => { f.Material.primary = "#010203"; return f.Material.primary })
        t("containerStyle write", () => { f.Material.containerStyle = Material.Filled; return f.Material.containerStyle === Material.Filled })
        t("theme write", () => { f.Material.theme = Material.Dark; return f.Material.theme === Material.Dark })
        t("roundedScale write", () => { f.Material.roundedScale = Material.SmallScale; return f.Material.roundedScale === Material.SmallScale })
        t("new", () => { f.Material.foo = 1; return f.Material.foo })
        t("new strict", () => { "use strict"; f.Material.foo2 = 1; return f.Material.foo2 })
        t("new [k]", () => { var k = "fo" + "o3"; f.Material[k] = 1; return f.Material[k] })
        t("item member", () => { f.Material.width = 3; return f.Material.width })
        t("read-only", () => { f.Material.primaryColor = "red"; return typeof f.Material.primaryColor })
        t("signal", () => { f.Material.accentChanged = 5; return typeof f.Material.accentChanged })
        t("handler", () => { f.Material.onAccentChanged = 5; return typeof f.Material.onAccentChanged })
        t("enum", () => { f.Material.Outlined = 5; return f.Material.Outlined })
        t("enum strict", () => { "use strict"; f.Material.Filled = 5; return f.Material.Filled })
        t("type enum", () => { Material.Dark = 9; return Material.Dark })
    }
}
