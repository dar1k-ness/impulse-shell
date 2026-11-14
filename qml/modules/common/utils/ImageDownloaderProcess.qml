import Impulse
import QtQuick
import Rustshell
import Rustshell.Io
import ri.modules.common
import ri.modules.common.functions

ImageDownload {
    id: root

    signal done(string path, int width, int height);
    required property string filePath;
    required property string sourceUrl;
    property string downloadUserAgent: Config.options?.networking.userAgent ?? ""

    running: true
    url: root.sourceUrl
    path: FileUtils.trimFileProtocol(root.filePath)
    userAgent: root.downloadUserAgent
    stdout: StdioCollector {
        id: imageSizeOutputCollector
        onStreamFinished: {
            const output = imageSizeOutputCollector.text.trim();
            const match = output.match(/(\d+)\s*x\s*(\d+)/);

            if (match) {
                const width = Number(match[1]);
                const height = Number(match[2]);
                root.done(root.filePath, width, height);
            }
        }
    }
}
