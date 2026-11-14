// What the wallpaper search of a background widget reads as it starts
// (qml/modules/rusty/background/widgets/AbstractBackgroundWidget.qml, the file of this tree).
//
// qt: made before the screen is measured
// qt: width known
// qt: search starts: screen 1920 1080 busiest false image /wallpaper.png
// qt: height known
// qt: search starts: screen 1920 1080 busiest true image /wallpaper.png
// qt: most busy asked for
// qt: search starts: screen 1920 1080 busiest true image /other.png
// qt: wallpaper changed
import QtQuick
import ri.modules.common
import ri.modules.rusty.background.widgets

Item {
    id: main
    property int screenW: 0
    property int screenH: 0

    // Made before its screen is measured, as a widget of the desktop container is.
    AbstractBackgroundWidget {
        configEntryName: "clock"
        screenWidth: main.screenW
        screenHeight: main.screenH
        scaledScreenWidth: main.screenW
        scaledScreenHeight: main.screenH
        wallpaperScale: 1
    }

    Timer {
        interval: 1
        running: true
        onTriggered: {
            console.log("made before the screen is measured");
            main.screenW = 1920;
            console.log("width known");
            main.screenH = 1080;
            console.log("height known");
            Config.options.background.widgets.clock.placementStrategy = "mostBusy";
            console.log("most busy asked for");
            Config.options.background.wallpaperPath = "/other.png";
            console.log("wallpaper changed");
            console.log("END");
            Qt.quit();
        }
    }
}
