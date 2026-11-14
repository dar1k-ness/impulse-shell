pragma Singleton
pragma ComponentBehavior: Bound

// From https://github.com/caelestia-dots/shell with modifications.
// License: GPLv3

import ri.modules.common
import ri.modules.common.functions
import Rustshell
import Rustshell.Io
import Rustshell.Hyprland
import Impulse
import QtQuick

/**
 * For managing brightness of monitors. Supports both brightnessctl and ddcutil.
 */
Singleton {
    id: root
    signal brightnessChanged()

    property var ddcMonitors: []
    readonly property list<BrightnessMonitor> monitors: Rustshell.screens.map(screen => monitorComp.createObject(root, {
        screen
    }))

    function getMonitorForScreen(screen: ShellScreen): var {
        return monitors.find(m => m.screen === screen);
    }

    function increaseBrightness(): void {
        // if gamma is not yet 100, first increase gamma
        if (Hyprsunset.gamma !== 100) {
            Hyprsunset.setGamma(Hyprsunset.gamma + 5);
            return;
        }

        const focusedName = Hyprland.focusedMonitor.name;
        const monitor = monitors.find(m => focusedName === m.screen.name);
        if (monitor)
            monitor.setBrightness(monitor.brightness + 0.05);
    }

    function decreaseBrightness(): void {
        const focusedName = Hyprland.focusedMonitor.name;
        const monitor = monitors.find(m => focusedName === m.screen.name);
        if (monitor && monitor.brightness > 0) 
            monitor.setBrightness(monitor.brightness - 0.05);
        // if brightness is 0, then decrease gamma
        else {
            Hyprsunset.setGamma(Hyprsunset.gamma - 5);
        }
    }

    reloadableId: "brightness"

    onMonitorsChanged: {
        ddcMonitors = [];
        DisplayBrightness.detectDdc();
    }

    function initializeMonitor(i: int): void {
        if (i >= monitors.length)
            return;
        monitors[i].initialize();
    }

    function ddcDetectFinished(): void {
        initializeMonitor(0);
    }

    Connections {
        target: DisplayBrightness
        function onDdcDetected(monitors) {
            root.ddcMonitors = monitors;
            root.ddcDetectFinished();
        }
    }

    component BrightnessMonitor: QtObject {
        id: monitor

        required property ShellScreen screen
        property bool isDdc
        property string busNum
        property int rawMaxBrightness: 100
        property real brightness
        property real brightnessMultiplier: 1.0
        property real multipliedBrightness: Math.max(0, Math.min(1, brightness * (Config.options.light.antiFlashbang.enable ? brightnessMultiplier : 1)))
        property bool ready: false
        property bool animateChanges: !monitor.isDdc
        // The backlight level last taken from outside the shell, with the multiplier it met.
        property var externalLevel: null

        onBrightnessChanged: {
            if (!monitor.ready) return;
            root.brightnessChanged();
        }

        Behavior on multipliedBrightness {
            enabled: monitor.animateChanges
            NumberAnimation {
                duration: 200
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Appearance.animationCurves.expressiveEffects
            }
        }
        onMultipliedBrightnessChanged: {
            if (monitor.animateChanges) syncBrightness();
            else setTimer.restart();
        }

        function initialize() {
            monitor.ready = false;
            const match = root.ddcMonitors.find(m => m.name === screen.name && !root.monitors.slice(0, root.monitors.indexOf(this)).some(mon => mon.busNum === m.busNum));
            isDdc = !!match;
            busNum = match?.busNum ?? "";
            if (isDdc) DisplayBrightness.readDdc(monitor, busNum);
            else DisplayBrightness.readBacklight(monitor);
        }

        readonly property Connections levels: Connections {
            target: DisplayBrightness
            function onLevelRead(m, ok, current, max) {
                if (m !== monitor) return;
                if (ok) {
                    monitor.rawMaxBrightness = max;
                    monitor.brightness = current / monitor.rawMaxBrightness;
                    monitor.ready = true;
                }
                root.initializeMonitor(root.monitors.indexOf(monitor) + 1);
            }
            // The backlight was set outside the shell (a brightnessctl keybind, the kernel).
            function onBacklightChanged(current, max) {
                if (!monitor.ready || monitor.isDdc) return;
                monitor.rawMaxBrightness = max;
                const value = current / monitor.rawMaxBrightness;
                if (monitor.brightness === value) return;
                monitor.externalLevel = { brightness: value, multiplier: monitor.brightnessMultiplier };
                monitor.brightness = value;
            }
        }

        // We need a delay for DDC monitors because they can be quite slow and might act weird with rapid changes
        property var setTimer: Timer {
            id: setTimer
            interval: monitor.isDdc ? 300 : 0
            onTriggered: {
                syncBrightness();
            }
        }

        function syncBrightness() {
            const brightnessValue = Math.max(monitor.multipliedBrightness, 0);
            if (isDdc) {
                DisplayBrightness.setDdc(busNum, brightnessValue, monitor.rawMaxBrightness);
            } else {
                // A level that came from outside is not written back rounded.
                const external = monitor.externalLevel;
                if (external && external.brightness === monitor.brightness && external.multiplier === monitor.brightnessMultiplier) return;
                monitor.externalLevel = null;
                DisplayBrightness.setBacklight(brightnessValue); // whole percent; raw 1 instead of 0%: never fully black
            }
        }

        function setBrightness(value: real): void {
            value = Math.max(0, Math.min(1, value));
            monitor.brightness = value;
        }

        function setBrightnessMultiplier(value: real): void {
            monitor.brightnessMultiplier = value;
        }
    }

    Component {
        id: monitorComp

        BrightnessMonitor {}
    }

    // Anti-flashbang
    property int workspaceAnimationDelay: 500
    property int contentSwitchDelay: 30
    function brightnessMultiplierForLightness(x: real): real {
        // I hand picked some values and fitted an exponential curve for this
        // 6.600135 + 216.360356 * e^(-0.0811129189x)
        // Division by 100 is to normalize to [0, 1]
        return (6.600135 + 216.360356 * Math.pow(Math.E, -0.0811129189 * x)) / 100.0;
    }
    Variants {
        model: Rustshell.screens
        Scope {
            id: screenScope
            required property var modelData
            property string screenName: modelData.name
            Connections {
                enabled: Config.options.light.antiFlashbang.enable && Appearance.m3colors.darkmode
                target: Hyprland
                function onRawEvent(event) {
                    if (["activewindowv2", "windowtitlev2"].includes(event.name)) {
                        screenshotTimer.interval = root.contentSwitchDelay;
                        screenshotTimer.restart();
                    } else if (["workspacev2"].includes(event.name)) {
                        screenshotTimer.interval = root.workspaceAnimationDelay;
                        screenshotTimer.restart();
                    }
                }
            }

            Timer {
                id: screenshotTimer
                interval: 700 // This is what I have for a Hyprland ws anim
                onTriggered: Screenshot.measureLightness(screenScope.screenName)
            }

            Connections {
                target: Screenshot
                function onLightnessMeasured(output, lightness) {
                    if (output !== screenScope.screenName)
                        return;
                    const newMultiplier = root.brightnessMultiplierForLightness(lightness)
                    Brightness.getMonitorForScreen(screenScope.modelData).setBrightnessMultiplier(newMultiplier)
                }
            }
        }
    }

    // External trigger points

    IpcHandler {
        target: "brightness"

        function increment() {
            onPressed: root.increaseBrightness()
        }

        function decrement() {
            onPressed: root.decreaseBrightness()
        }
    }

    GlobalShortcut {
        name: "brightnessIncrease"
        description: "Increase brightness"
        onPressed: root.increaseBrightness()
    }

    GlobalShortcut {
        name: "brightnessDecrease"
        description: "Decrease brightness"
        onPressed: root.decreaseBrightness()
    }
}
