pragma Singleton

import QtQuick
import ri.modules.common
import Rustshell
import Rustshell.Io
import Rustshell.Hyprland
import Impulse

/**
 * Simple hyprsunset service with automatic mode.
 * In theory we don't need this because hyprsunset has a config file, but it somehow doesn't work.
 */
Singleton {
    id: root
    signal gammaChangeAttempt()

    readonly property real gammaLowerLimit: 25

    property string from: Config.options?.light?.night?.from ?? "19:00" 
    property string to: Config.options?.light?.night?.to ?? "06:30"
    property bool configReady: Config?.ready ?? true
    property bool automatic: Config.options?.light?.night?.automatic && configReady
    property int colorTemperature: Config.options?.light?.night?.colorTemperature ?? 5000
    property int defaultColorTemperature: 6000
    property int gamma: 100
    property bool shouldBeOn
    property bool firstEvaluation: true
    property bool temperatureActive: false
    property bool autoDarkMode: (Config.options?.appearance?.autoDarkMode ?? false) && configReady

    property int fromHour: Number(from.split(":")[0])
    property int fromMinute: Number(from.split(":")[1])
    property int toHour: Number(to.split(":")[0])
    property int toMinute: Number(to.split(":")[1])

    property int clockHour: DateTime.clock.hours
    property int clockMinute: DateTime.clock.minutes

    property var manualActive
    property int manualActiveHour
    property int manualActiveMinute

    onClockMinuteChanged: reEvaluate()
    onAutomaticChanged: {
        root.manualActive = undefined;
        root.firstEvaluation = true;
        reEvaluate();
    }

    function inBetween(t, from, to) {
        if (from < to) {
            return (t >= from && t <= to);
        } else {
            // Wrapped around midnight
            return (t >= from || t <= to);
        }
    }

    function reEvaluate() {
        const t = clockHour * 60 + clockMinute;
        const from = fromHour * 60 + fromMinute;
        const to = toHour * 60 + toMinute;
        const manualActive = manualActiveHour * 60 + manualActiveMinute;

        if (root.manualActive !== undefined && (inBetween(from, manualActive, t) || inBetween(to, manualActive, t))) {
            root.manualActive = undefined;
        }
        root.shouldBeOn = inBetween(t, from, to);
        if (firstEvaluation) {
            firstEvaluation = false;
            root.ensureState();
            root.ensureThemeMode();
        }
    }

    onShouldBeOnChanged: {
        root.ensureState();
        root.ensureThemeMode();
    }
    onAutoDarkModeChanged: {
        root.reEvaluate();
        root.ensureThemeMode();
    }

    // Auto theme: dark while the night schedule is on, light otherwise
    function ensureThemeMode() {
        if (!root.autoDarkMode)
            return;
        const dark = root.shouldBeOn;
        if (Appearance.m3colors.darkmode === dark)
            return;
        Switchwall.run({ mode: dark ? "dark" : "light", noSwitch: true });
    }
    // Reads the config, not `automatic`: Qt runs this handler before it re-evaluates the bindings
    // that read `configReady`, so `automatic` is still false here.
    onConfigReadyChanged: if (!Config.options?.light?.night?.automatic) root.ensureState()
    function ensureState() {
        // console.log("[Hyprsunset] Ensuring state:", root.shouldBeOn, "Automatic mode:", root.automatic);
        if (root.manualActive !== undefined)
            return;
        if (!root.automatic) {
            // Not managed by the schedule: don't leave a stale temperature from before a reload
            if (root.configReady)
                root.disableTemperature();
            return;
        }
        if (root.shouldBeOn) {
            root.enableTemperature();
        } else {
            root.disableTemperature();
        }
    }

    function startHyprsunset() {
        daemon.start();
    }

    function load() {
        root.startHyprsunset();
        root.ensureState();
        root.setGamma(root.gamma);
    }

    function enableTemperature() {
        root.temperatureActive = true;

        // console.log("[Hyprsunset] Enabling");
        root.startHyprsunset();
        daemon.setTemperature(root.colorTemperature);
    }

    function disableTemperature() {
        root.temperatureActive = false;
        // console.log("[Hyprsunset] Disabling");
        daemon.setTemperature(root.defaultColorTemperature);
    }

    function setGamma(gamma) {
        root.gamma = Math.max(root.gammaLowerLimit, Math.min(100, gamma));

        root.gammaChangeAttempt();

        root.startHyprsunset();
        daemon.setGamma(root.gamma);
    }

    function fetchState() {
        daemon.readTemperature();
    }

    Component.onCompleted: root.fetchState()

    HyprsunsetClient {
        id: daemon
        onTemperatureRead: text => {
            const output = text.trim();
            if (output.length == 0 || output.startsWith("Couldn't"))
                root.temperatureActive = false;
            else
                root.temperatureActive = (output != root.defaultColorTemperature); // 6000 is the default when off
            // console.log("[Hyprsunset] Fetched state:", output, "->", root.temperatureActive);
        }
    }

    function toggleTemperature(active = undefined) {
        if (root.manualActive === undefined) {
            root.manualActive = root.temperatureActive;
            root.manualActiveHour = root.clockHour;
            root.manualActiveMinute = root.clockMinute;
        }

        root.manualActive = active !== undefined ? active : !root.manualActive;
        if (root.manualActive) {
            root.enableTemperature();
        } else {
            root.disableTemperature();
        }
    }

    // Change temp
    Connections {
        target: Config.options.light.night
        function onColorTemperatureChanged() {
            if (!root.temperatureActive) return;
            daemon.setTemperature(Config.options.light.night.colorTemperature);
        }
    }
}