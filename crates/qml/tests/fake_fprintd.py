#!/usr/bin/env python3
"""Fake fprintd (net.reactivated.Fprint) for the qml crate's fingerprint tests.

usage: fake_fprintd.py CONFIG_JSON
CONFIG: {"manager_error": bool, "devices": [{"fingers": [...]} | {"error": "NoEnrolledPrints"}]}
Prints READY once owned, then "LIST <user>" for every ListEnrolledFingers call and "SYNC" for
every net.reactivated.Fprint.Test.Sync call on the manager (calls are handled in order).
"""
import json
import sys

import dbus
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

PROPS = "org.freedesktop.DBus.Properties"
DEV = "net.reactivated.Fprint.Device"
cfg = json.loads(sys.argv[1])
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.SystemBus()


def fail(err):
    raise dbus.exceptions.DBusException("fake", name="net.reactivated.Fprint.Error." + err)


class Device(dbus.service.Object):
    def __init__(self, i, d):
        super().__init__(bus, "/net/reactivated/Fprint/Device/%d" % i)
        self.d = d
        self.props = {"name": "Fake Reader %d" % i, "scan-type": "press",
                      "num-enroll-stages": dbus.Int32(5), "finger-present": False,
                      "finger-needed": False}

    @dbus.service.method(DEV, in_signature="s", out_signature="as")
    def ListEnrolledFingers(self, user):
        print("LIST", user, flush=True)
        if "error" in self.d:
            fail(self.d["error"])
        return dbus.Array(self.d.get("fingers", []), signature="s")

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.props[name]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.props if iface == DEV else {}


class Manager(dbus.service.Object):
    def __init__(self, devs):
        super().__init__(bus, "/net/reactivated/Fprint/Manager")
        self.devs = devs

    @dbus.service.method("net.reactivated.Fprint.Manager", out_signature="ao")
    def GetDevices(self):
        if cfg.get("manager_error"):
            fail("Internal")
        return dbus.Array([d._object_path for d in self.devs], signature="o")

    @dbus.service.method("net.reactivated.Fprint.Manager", out_signature="o")
    def GetDefaultDevice(self):
        if not self.devs:
            fail("NoSuchDevice")
        return self.devs[0]._object_path

    @dbus.service.method("net.reactivated.Fprint.Test")
    def Sync(self):
        print("SYNC", flush=True)


devs = [Device(i, d) for i, d in enumerate(cfg.get("devices", []))]
mgr = Manager(devs)
name = dbus.service.BusName("net.reactivated.Fprint", bus)
print("READY", flush=True)
GLib.MainLoop().run()
