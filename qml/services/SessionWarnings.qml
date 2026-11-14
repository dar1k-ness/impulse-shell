pragma Singleton

import ri.modules.common
import ri.modules.common.functions
import QtQuick
import Rustshell
import Impulse

Singleton {
    id: root

    property bool packageManagerRunning: false
    property bool downloadRunning: false

    readonly property list<string> packageManagers: ["yay", "paru", "dnf", "zypper", "apt", "apx", "xbps", "snap", "apk", "yum", "epsi", "pikman"]
    readonly property string pacmanDatabase: "/var/lib/pacman"
    readonly property list<string> downloaders: ["curl", "wget", "aria2c", "yt-dlp"]
    readonly property string downloads: `${Rustshell.env("HOME")}/Downloads`

    // The answers of the last refresh; an older one is ignored
    property int packageManagerQuery: -1
    property int pacmanLockQuery: -1
    property int downloaderQuery: -1
    property int partialDownloadQuery: -1

    function refresh() {
        packageManagerRunning = false;
        downloadRunning = false;
        packageManagerQuery = Processes.query(packageManagers);
        pacmanLockQuery = Files.list(pacmanDatabase);
        downloaderQuery = Processes.query(downloaders);
        partialDownloadQuery = Files.list(downloads);
    }

    Connections {
        target: Processes
        function onAnswered(id, running) {
            if (running.length === 0) return;
            if (id === root.packageManagerQuery) root.packageManagerRunning = true;
            if (id === root.downloaderQuery) root.downloadRunning = true;
        }
    }

    Connections {
        target: Files
        function onListed(id, names) {
            if (id === root.pacmanLockQuery && names.includes("db.lck")) {
                root.packageManagerRunning = true;
            }
            if (id === root.partialDownloadQuery && names.some(name => name.endsWith(".crdownload") || name.endsWith(".part"))) {
                root.downloadRunning = true;
            }
        }
    }
}
