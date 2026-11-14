pragma ComponentBehavior: Bound
pragma Singleton
import ri.modules.common
import ri.modules.common.utils
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.services
import QtQuick
import QtQuick.Controls
import Qt.labs.synchronizer
import Rustshell
import Impulse

Singleton {
    id: root

    enum Action {
        Copy,
        Edit,
        Search,
        CharRecognition,
        Record,
        RecordWithSound
    }

    property string imageSearchEngineBaseUrl: Config.options.search.imageSearch.imageSearchEngineBaseUrl
    property string fileUploadApiEndpoint: "https://uguu.se/upload"

    /**
     * Runs `action` on the region of the screen capture at `screenshotPath`.
     * The capture file is removed when the action is done.
     */
    function run(x, y, width, height, screenshotPath, action, saveDir = "") {
        const rx = Math.round(x);
        const ry = Math.round(y);
        const rw = Math.round(width);
        const rh = Math.round(height);
        switch (action) {
            case ScreenshotAction.Action.Copy:
                Screenshot.copy(screenshotPath, rx, ry, rw, rh, saveDir);
                break;
            case ScreenshotAction.Action.Edit:
                Screenshot.edit(screenshotPath, rx, ry, rw, rh, Config.options.regionSelector.annotation.useSatty ? "satty" : "swappy");
                break;
            case ScreenshotAction.Action.Search:
                Screenshot.search(screenshotPath, rx, ry, rw, rh, root.fileUploadApiEndpoint, root.imageSearchEngineBaseUrl);
                break;
            case ScreenshotAction.Action.CharRecognition:
                Screenshot.recognizeText(screenshotPath, rx, ry, rw, rh);
                break;
            case ScreenshotAction.Action.Record:
                ScreenRecorder.toggleRegion(`${rx},${ry} ${rw}x${rh}`, false, Config.options.screenRecord.savePath);
                break;
            case ScreenshotAction.Action.RecordWithSound:
                ScreenRecorder.toggleRegion(`${rx},${ry} ${rw}x${rh}`, true, Config.options.screenRecord.savePath);
                break;
            default:
                console.warn("[Region Selector] Unknown snip action, skipping snip.");
                return;
        }
    }
}
