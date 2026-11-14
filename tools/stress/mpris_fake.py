#!/usr/bin/env python3
# mpris_fake.py NAME PERIOD: org.mpris.MediaPlayer2.NAME, switches track / playback status every PERIOD s
import sys
from gi.repository import Gio, GLib
name, period = sys.argv[1], float(sys.argv[2])
XML = """<node>
<interface name="org.mpris.MediaPlayer2"><property name="Identity" type="s" access="read"/><property name="DesktopEntry" type="s" access="read"/>
<property name="CanQuit" type="b" access="read"/><property name="CanRaise" type="b" access="read"/><method name="Raise"/><method name="Quit"/></interface>
<interface name="org.mpris.MediaPlayer2.Player">
<method name="PlayPause"/><method name="Play"/><method name="Pause"/><method name="Stop"/><method name="Next"/><method name="Previous"/>
<method name="Seek"><arg type="x" direction="in"/></method><method name="SetPosition"><arg type="o" direction="in"/><arg type="x" direction="in"/></method>
<property name="PlaybackStatus" type="s" access="read"/><property name="Metadata" type="a{sv}" access="read"/><property name="Position" type="x" access="read"/>
<property name="Volume" type="d" access="readwrite"/><property name="Rate" type="d" access="read"/>
<property name="CanGoNext" type="b" access="read"/><property name="CanGoPrevious" type="b" access="read"/><property name="CanPlay" type="b" access="read"/>
<property name="CanPause" type="b" access="read"/><property name="CanSeek" type="b" access="read"/><property name="CanControl" type="b" access="read"/>
<signal name="Seeked"><arg type="x"/></signal></interface></node>"""
st = {"n": 0}
def meta():
    n = st["n"]
    return GLib.Variant("a{sv}", {"mpris:trackid": GLib.Variant("o", f"/stress/track{n}"), "mpris:length": GLib.Variant("x", 180000000),
        "xesam:title": GLib.Variant("s", f"{name} track {n}"), "xesam:artist": GLib.Variant("as", [f"Artist {n % 7}"]),
        "xesam:album": GLib.Variant("s", f"Album {n % 3}")})
def prop(iface, p):
    return {"Identity": GLib.Variant("s", name), "DesktopEntry": GLib.Variant("s", name), "CanQuit": GLib.Variant("b", True),
        "CanRaise": GLib.Variant("b", False), "PlaybackStatus": GLib.Variant("s", "Playing" if st["n"] % 4 else "Paused"),
        "Metadata": meta(), "Position": GLib.Variant("x", (st["n"] * 1000000) % 180000000), "Volume": GLib.Variant("d", 1.0),
        "Rate": GLib.Variant("d", 1.0)}.get(p, GLib.Variant("b", True))
def call(conn, sender, path, iface, method, params, inv): inv.return_value(None)
def get(conn, sender, path, iface, p): return prop(iface, p)
def tick(conn):
    st["n"] += 1
    conn.emit_signal(None, "/org/mpris/MediaPlayer2", "org.freedesktop.DBus.Properties", "PropertiesChanged",
        GLib.Variant("(sa{sv}as)", ("org.mpris.MediaPlayer2.Player", {"Metadata": meta(), "PlaybackStatus": prop(None, "PlaybackStatus")}, [])))
    return True
def acquired(conn, _):
    node = Gio.DBusNodeInfo.new_for_xml(XML)
    for i in node.interfaces: conn.register_object("/org/mpris/MediaPlayer2", i, call, get, None)
    GLib.timeout_add(int(period * 1000), tick, conn)
Gio.bus_own_name(Gio.BusType.SESSION, "org.mpris.MediaPlayer2." + name, 0, acquired, None, None)
GLib.MainLoop().run()
