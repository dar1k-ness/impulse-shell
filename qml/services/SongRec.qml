pragma Singleton
pragma ComponentBehavior: Bound

import ri.modules.common
import Impulse
import QtQuick
import Rustshell

Singleton {
    id: root

    enum MonitorSource { Monitor, Input }

    property var monitorSource: SongRec.MonitorSource.Monitor
    property int timeoutInterval: Config.options.musicRecognition.interval
    property int timeoutDuration: Config.options.musicRecognition.timeout
    readonly property bool running: MusicRecognizer.running

    function toggleRunning(running) {
        if (running ?? !root.running) {
            if (!root.running) MusicRecognizer.start(root.timeoutInterval, root.timeoutDuration, root.monitorSourceString)
        } else {
            MusicRecognizer.stop()
        }
        root.recognizedNote = -1
    }

    function toggleMonitorSource(source) {
        if (source !== undefined) {
            root.monitorSource = source
            return
        }
        root.monitorSource = (root.monitorSource === SongRec.MonitorSource.Monitor) ? SongRec.MonitorSource.Input : SongRec.MonitorSource.Monitor
    }
    function monitorSourceToString(source) {
        if (source === SongRec.MonitorSource.Monitor) {
            return "monitor"
        } else {
            return "input"
        }
    }
    readonly property string monitorSourceString: monitorSourceToString(monitorSource)
    property var recognizedTrack: ({ title:"", subtitle:"", url:""})
    // What Notify.send returned for the "Music Recognized" notification whose buttons count.
    property int recognizedNote: -1

    function handleRecognition(jsonText) {
        try {
            var obj = JSON.parse(jsonText)
            root.recognizedTrack = {
                title: obj.track.title,
                subtitle: obj.track.subtitle,
                url: obj.track.url
            }
            root.recognizedNote = Notify.send(
                Translation.tr("Music Recognized"),
                root.recognizedTrack.title + " - " + root.recognizedTrack.subtitle,
                { actions: ["Shazam", "YouTube"] }
            )
        } catch(e) {
            Notify.send(Translation.tr("Couldn't recognize music"), Translation.tr("Perhaps what you're listening to is too niche"))
        }
    }

    Connections {
        target: MusicRecognizer
        function onFinished(text, exitCode) {
            // 1: nothing listened (songrec is not installed), so there is no answer to read
            if (exitCode === 1) {
                Notify.send(Translation.tr("Couldn't recognize music"), Translation.tr("Make sure you have songrec installed"))
                return
            }
            root.handleRecognition(text)
        }
    }

    Connections {
        target: Notify
        // The buttons in the order they were given: 0 is Shazam, 1 is YouTube.
        function onActionInvoked(id, index) {
            if (id !== root.recognizedNote) return
            if (index === 0) {
                Qt.openUrlExternally(root.recognizedTrack.url);
            } else {
                Qt.openUrlExternally("https://www.youtube.com/results?search_query=" + root.recognizedTrack.title + " - " + root.recognizedTrack.subtitle);
            }
        }
    }
}
