pragma ComponentBehavior: Bound
import QtQuick
import Rustshell
import Rustshell.Io
import Impulse
import ri.modules.common.functions
import ri.services
import ri.modules.common
import ".."

GCloudApi {
    id: root

    property string uploadEndpoint: "https://uguu.se/upload"

    property bool tokenReady: GoogleCloud.tokenReady
    property bool onlineImageReady: false
    readonly property bool preparationReady: tokenReady && onlineImageReady

    function annotateImage(imageUri: string) {
        resetState();
        root.state = GCloudApi.State.Preparing;
        root.onlineImageReady = false
        GoogleCloud.load();

        lookMultiproc.image = FileUtils.trimFileProtocol(imageUri);
        root.onlineImageReady = true;
    }

    onPreparationReadyChanged: {
        if (!preparationReady) return;
        if (GoogleCloud.tokenError || GoogleCloud.keyError) {
            root.state = GCloudApi.State.Error;
            root.error(Translation.tr("Set your Google Cloud service account key"));
            return;
        }
        root.state = GCloudApi.State.Processing;
        lookMultiproc.token = GoogleCloud.token;
        lookMultiproc.project = GoogleCloud.projectId;
        lookMultiproc.running = false;
        lookMultiproc.running = true;
    }

    GoogleVision {
        id: lookMultiproc
        stdout: StdioCollector {
            onStreamFinished: root.handleApiOutput(this.text)
        }
    }
}
