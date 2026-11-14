pragma ComponentBehavior: Bound
import QtQuick
import Rustshell.Io
import Impulse
import ri.services
import ".."

GCloudApi {
    id: root

    property list<string> pendingStrings
    property bool setupReady: false
    readonly property bool preparationReady: GoogleCloud.tokenReady && setupReady

    function translateStrings(strings: list<string>) {
        GoogleCloud.load();
        root.setupReady = false;
        root.pendingStrings = strings;
        root.state = GCloudApi.State.Preparing;
        root.setupReady = true;
    }

    onPreparationReadyChanged: {
        if (!preparationReady) return;
        root.state = GCloudApi.State.Processing;

        multiproc.token = GoogleCloud.token;
        multiproc.project = GoogleCloud.projectId;
        multiproc.language = Translation.languageCode;
        multiproc.strings = root.pendingStrings;
        multiproc.running = false;
        multiproc.running = true;
    }

    GoogleTranslate {
        id: multiproc
        stdout: StdioCollector {
            onStreamFinished: root.handleApiOutput(this.text)
        }
    }
}
