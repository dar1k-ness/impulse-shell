import QtQuick
import Rustshell
import Rustshell.Io
import Impulse
import ri.modules.common
import ri.modules.common.functions

// From https://github.com/caelestia-dots/shell with modifications.
// License: GPLv3

StyledImage {
    id: root
    required property var fileModelData
    asynchronous: true
    fillMode: Image.PreserveAspectFit

    source: {
        if (!fileModelData.fileIsDir)
            return Rustshell.iconPath("application-x-zerosize");

        if ([Directories.documents, Directories.downloads, Directories.music, Directories.pictures, Directories.videos].some(dir => FileUtils.trimFileProtocol(dir) === fileModelData.filePath))
            return Rustshell.iconPath(`folder-${fileModelData.fileName.toLowerCase()}`);

        return Rustshell.iconPath("inode-directory");
    }

    onStatusChanged: {
        if (status === Image.Error)
            source = Rustshell.iconPath("error");
    }

    FileMimeType {
        running: !fileModelData.fileIsDir
        path: fileModelData.filePath
        followSymlinks: false
        stdout: StdioCollector {
            onStreamFinished: {
                const mime = text.replace("/", "-");
                root.source = Images.validImageTypes.some(t => mime === `image-${t}`) ? fileModelData.fileUrl : Rustshell.iconPath(mime, "image-missing");
            }
        }
    }
}
