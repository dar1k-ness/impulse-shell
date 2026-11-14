#!/usr/bin/env python3
"""Fake UPower / power-profiles-daemon / BlueZ for the qml crate's system-bus tests.

usage: fake_sysbus.py MODE INITIAL_JSON     MODE: upower | ppd | ppd-legacy | bluez | nm | notifd | polkitd
Commands arrive as JSON lines on stdin; events (writes/calls received) are printed on stdout.
"""
import json
import sys

import dbus
import dbus.lowlevel
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

PROPS = "org.freedesktop.DBus.Properties"
OM = "org.freedesktop.DBus.ObjectManager"
U32 = {"Type", "State", "WarningLevel", "BatteryLevel", "Technology",
       "DiscoverableTimeout", "PairableTimeout", "Class", "DeviceType"}
I64 = {"TimeToEmpty", "TimeToFull", "UpdateTime"}
DICT_LISTS = {"Profiles", "ActiveProfileHolds", "Actions"}


def say(*a):
    print(*a, flush=True)


def typed(iface, k, v):
    if isinstance(v, bool):
        return dbus.Boolean(v)
    if iface == "org.bluez.Battery1" and k == "Percentage":
        return dbus.Byte(v)
    if k in U32:
        return dbus.UInt32(v)
    if k in I64:
        return dbus.Int64(v)
    if k == "Adapter":
        return dbus.ObjectPath(v)
    if k == "Devices":
        return dbus.Array([dbus.ObjectPath(p) for p in v], signature="o")
    if isinstance(v, (int, float)):
        if iface.startswith("org.freedesktop.UPower") or isinstance(v, float):
            return dbus.Double(v)
        return dbus.Int32(v)
    if isinstance(v, list):
        if k in DICT_LISTS:
            return dbus.Array(
                [dbus.Dictionary({a: dbus.String(b) for a, b in d.items()}, signature="sv")
                 for d in v], signature="a{sv}")
        return dbus.Array([dbus.String(x) for x in v], signature="s")
    return dbus.String(v)


def typed_props(iface, props):
    return dbus.Dictionary({k: typed(iface, k, v) for k, v in props.items()}, signature="sv")


class Obj(dbus.service.Object):
    def __init__(self, bus, path, ifaces, svc):
        super().__init__(bus, path)
        self.path = path
        self.svc = svc
        self.ifaces = {i: typed_props(i, p) for i, p in ifaces.items()}

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        say("GETALL", self.path, iface)
        return self.ifaces.get(str(iface), dbus.Dictionary({}, signature="sv"))

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v",
                         async_callbacks=("ok", "err"))
    def Get(self, iface, prop, ok, err):
        say("GET", self.path, iface, prop)
        v = self.ifaces[str(iface)][str(prop)]
        if self.svc.hold_get == str(prop):
            # "hold-get": answered on "release-get".
            self.svc.hold_get = None
            self.svc.held_get = lambda: ok(v)
            say("HELD GET", prop)
            return
        ok(v)

    @dbus.service.method(PROPS, in_signature="ssv", out_signature="",
                         async_callbacks=("ok", "err"))
    def Set(self, iface, prop, value, ok, err):
        iface, prop = str(iface), str(prop)
        if isinstance(value, dbus.Boolean):
            value = bool(value)
        say("SET", self.path, prop, json.dumps(value))
        if self.svc.hold("hold_set", "SET", prop, ok):
            return
        if self.svc.flags.get("reject_set"):
            err(dbus.exceptions.DBusException("org.freedesktop.DBus.Error.AccessDenied"))
            return
        changes = {prop: value}
        if prop == "Powered":
            changes["PowerState"] = "on" if value else "off"
        if prop == "ActiveProfile":
            changes["ActiveProfileHolds"] = []
        self.update(iface, {k: v for k, v in changes.items()})
        ok()

    # org.freedesktop.NetworkManager.Device.Wireless
    @dbus.service.method("org.freedesktop.NetworkManager.Device.Wireless", in_signature="a{sv}",
                         out_signature="", async_callbacks=("ok", "err"))
    def RequestScan(self, options, ok, err):
        say("SCAN", self.path)
        if not self.svc.hold("hold_scan", "RequestScan", self.path, ok):
            ok()

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    def update(self, iface, props, invalidate=False):
        t = typed_props(iface, {k: (v if not isinstance(v, dbus.Boolean) else bool(v))
                                for k, v in props.items()})
        self.ifaces.setdefault(iface, dbus.Dictionary({}, signature="sv")).update(t)
        if invalidate:
            self.PropertiesChanged(iface, dbus.Dictionary({}, signature="sv"),
                                   dbus.Array(list(props), signature="s"))
        else:
            self.PropertiesChanged(iface, t, dbus.Array([], signature="s"))

    # org.bluez.Adapter1
    @dbus.service.method("org.bluez.Adapter1", in_signature="", out_signature="")
    def StartDiscovery(self):
        say("CALL", self.path, "StartDiscovery")
        self.update("org.bluez.Adapter1", {"Discovering": True})

    @dbus.service.method("org.bluez.Adapter1", in_signature="", out_signature="")
    def StopDiscovery(self):
        say("CALL", self.path, "StopDiscovery")
        self.update("org.bluez.Adapter1", {"Discovering": False})

    @dbus.service.method("org.bluez.Adapter1", in_signature="o", out_signature="")
    def RemoveDevice(self, dev):
        say("CALL", self.path, "RemoveDevice", str(dev))
        self.svc.remove(str(dev), None)

    # org.bluez.Device1
    def _dev(self, name, prop, value):
        say("CALL", self.path, name)
        if self.svc.flags.get("fail_" + name.lower()):
            raise dbus.exceptions.DBusException("org.bluez.Error.Failed")
        if prop:
            self.update("org.bluez.Device1", {prop: value})

    @dbus.service.method("org.bluez.Device1", in_signature="", out_signature="")
    def Connect(self):
        self._dev("Connect", "Connected", True)

    @dbus.service.method("org.bluez.Device1", in_signature="", out_signature="",
                         async_callbacks=("ok", "err"))
    def Disconnect(self, ok, err):
        # "hold_disconnect": the call stays open until "fail-held" answers it with an error.
        if self.svc.flags.get("hold_disconnect"):
            say("CALL", self.path, "Disconnect")
            self.svc.hold("hold_disconnect", "Disconnect", self.path, err)
            return
        try:
            self._dev("Disconnect", "Connected", False)
        except dbus.exceptions.DBusException as e:
            err(e)
            return
        ok()

    @dbus.service.method("org.bluez.Device1", in_signature="", out_signature="")
    def Pair(self):
        self._dev("Pair", "Paired", True)

    @dbus.service.method("org.bluez.Device1", in_signature="", out_signature="")
    def CancelPairing(self):
        self._dev("CancelPairing", None, None)


class UPowerRoot(Obj):
    @dbus.service.method("org.freedesktop.UPower", in_signature="", out_signature="ao")
    def EnumerateDevices(self):
        return dbus.Array([dbus.ObjectPath(p) for p in self.svc.objs if p != self.svc.display
                           and p != self.path], signature="o")

    @dbus.service.method("org.freedesktop.UPower", in_signature="", out_signature="o")
    def GetDisplayDevice(self):
        return dbus.ObjectPath(self.svc.display)

    @dbus.service.signal("org.freedesktop.UPower", signature="o")
    def DeviceAdded(self, path):
        pass

    @dbus.service.signal("org.freedesktop.UPower", signature="o")
    def DeviceRemoved(self, path):
        pass


class Manager(dbus.service.Object):
    def __init__(self, bus, svc, path="/"):
        super().__init__(bus, path)
        self.svc = svc

    @dbus.service.method(OM, in_signature="", out_signature="a{oa{sa{sv}}}")
    def GetManagedObjects(self):
        if self.svc.flags.get("fail_managed"):
            # A daemon that has the name and cannot give its objects yet.
            say("FAILED GetManagedObjects")
            raise dbus.exceptions.DBusException("not ready",
                                                name="org.freedesktop.DBus.Error.Failed")
        return dbus.Dictionary({dbus.ObjectPath(p): dbus.Dictionary(o.ifaces, signature="sa{sv}")
                                for p, o in self.svc.objs.items()}, signature="oa{sa{sv}}")

    @dbus.service.signal(OM, signature="oa{sa{sv}}")
    def InterfacesAdded(self, path, ifaces):
        pass

    @dbus.service.signal(OM, signature="oas")
    def InterfacesRemoved(self, path, ifaces):
        pass


class AgentManager(dbus.service.Object):
    def __init__(self, bus, svc):
        super().__init__(bus, "/org/bluez")
        self.svc = svc

    @dbus.service.method("org.bluez.AgentManager1", in_signature="os", out_signature="",
                         sender_keyword="sender")
    def RegisterAgent(self, path, cap, sender=None):
        if self.svc.agent == (sender, str(path)):
            raise dbus.exceptions.DBusException("Already Exists",
                                                name="org.bluez.Error.AlreadyExists")
        self.svc.agent = (sender, str(path))
        say("AGENT RegisterAgent", path, cap)

    @dbus.service.method("org.bluez.AgentManager1", in_signature="o", out_signature="")
    def RequestDefaultAgent(self, path):
        say("AGENT RequestDefaultAgent", path)

    @dbus.service.method("org.bluez.AgentManager1", in_signature="o", out_signature="",
                         async_callbacks=("ok", "err"))
    def UnregisterAgent(self, path, ok, err):
        say("AGENT UnregisterAgent", path)
        self.svc.agent = None
        if not self.svc.hold("hold_unregister_agent", "UnregisterAgent", path, ok):
            ok()


AGENT_SIGS = {"Release": "", "RequestPinCode": "o", "DisplayPinCode": "os",
              "RequestPasskey": "o", "DisplayPasskey": "ouq", "RequestConfirmation": "ou",
              "RequestAuthorization": "o", "AuthorizeService": "os", "Cancel": ""}
NOTIF = "org.freedesktop.Notifications"
LOGIN = "org.freedesktop.login1"
POLKIT = "org.freedesktop.PolicyKit1"
DBUS = "org.freedesktop.DBus"
POLKIT_AGENT = "org.freedesktop.PolicyKit1.AuthenticationAgent"


class Login(dbus.service.Object):
    """logind with one session, "c1", that every pid belongs to."""

    def __init__(self, bus):
        super().__init__(bus, "/org/freedesktop/login1")
        self.session = LoginSession(bus)

    @dbus.service.method(LOGIN + ".Manager", in_signature="u", out_signature="o")
    def GetSessionByPID(self, pid):
        return dbus.ObjectPath("/org/freedesktop/login1/session/c1")


class LoginSession(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, "/org/freedesktop/login1/session/c1")

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        if (iface, prop) != (LOGIN + ".Session", "Id"):
            raise dbus.exceptions.DBusException(
                prop, name="org.freedesktop.DBus.Error.UnknownProperty")
        return dbus.String("c1", variant_level=1)


class Authority(dbus.service.Object):
    """polkitd's Authority: one agent per session, as polkitd keeps it
    (polkit_backend_interactive_authority_register_authentication_agent refuses a second one
    for the subject; unregistering takes the sender and path that registered)."""

    def __init__(self, bus, svc, tag="POLKIT"):
        super().__init__(bus, "/org/freedesktop/PolicyKit1/Authority")
        self.svc = svc
        self.tag = tag
        self.registered = None

    @dbus.service.method(POLKIT + ".Authority", in_signature="(sa{sv})ss", out_signature="",
                         sender_keyword="sender")
    def RegisterAuthenticationAgent(self, subject, locale, path, sender=None):
        if self.registered is not None:
            say(self.tag + " Refused", path)
            raise dbus.exceptions.DBusException(
                "An authentication agent already exists for the given subject",
                name=POLKIT + ".Error.Failed")
        self.registered = self.svc.agent = (sender, str(path))
        say(self.tag + " Register", path, subject[0], subject[1].get("session-id", ""))

    @dbus.service.method(POLKIT + ".Authority", in_signature="(sa{sv})s", out_signature="",
                         sender_keyword="sender", async_callbacks=("ok", "err"))
    def UnregisterAuthenticationAgent(self, subject, path, ok, err, sender=None):
        if self.svc.hold_unregister:
            # polkitd busy: the call is handled on "release-unregister".
            self.svc.hold_unregister = False
            say(self.tag + " Unregister-held", path)
            self.svc.held = lambda: self.UnregisterAuthenticationAgent(subject, path, ok, err,
                                                                       sender)
            return
        if self.registered != (sender, str(path)):
            say(self.tag + " Unregister-unknown", path)
            err(dbus.exceptions.DBusException(
                "No such agent registered", name=POLKIT + ".Error.Failed"))
            return
        self.registered = None
        # The agent stays the target of "begin": a test may call the withdrawn object.
        say(self.tag + " Unregister", path)
        c = self.svc.begin_on_unregister
        if c is None:
            ok()
            return
        # polkitd starting a request while the agent unregisters: answered after that request.
        self.svc.begin_on_unregister = None
        self.svc.call_agent(c, done=ok)


class Notifd(dbus.service.Object):
    def __init__(self, bus, svc):
        super().__init__(bus, "/org/freedesktop/Notifications")
        self.next = 1
        self.svc = svc

    @dbus.service.method(NOTIF, in_signature="susssasa{sv}i", out_signature="u",
                         async_callbacks=("ok", "err"))
    def Notify(self, app, replaces, icon, summary, body, actions, hints, expire, ok, err):
        if self.svc.hold("hold_notify", "Notify", str(body), ok):
            return
        nid = int(replaces)
        if not nid:
            nid = self.next
            self.next += 1
        say("NOTIFY", nid, json.dumps({"app": str(app), "icon": str(icon),
                                       "summary": str(summary), "body": str(body),
                                       "actions": [str(a) for a in actions],
                                       "urgency": int(hints.get("urgency", -1)),
                                       "expire": int(expire)}))
        ok(dbus.UInt32(nid))

    @dbus.service.method(NOTIF, in_signature="u", out_signature="")
    def CloseNotification(self, nid):
        say("CLOSE", nid)
        self.NotificationClosed(nid, dbus.UInt32(3))

    @dbus.service.signal(NOTIF, signature="us")
    def ActionInvoked(self, nid, key):
        pass

    @dbus.service.signal(NOTIF, signature="uu")
    def NotificationClosed(self, nid, reason):
        pass


class Marker(dbus.service.Object):
    """Prints MARK when called: a call from a client after its earlier calls arrives behind them
    (the bus keeps one sender's messages in order), so the lines before MARK are all of them."""

    def __init__(self, bus):
        super().__init__(bus, "/org/test/Mark")

    @dbus.service.method("org.test.Mark", in_signature="", out_signature="")
    def Mark(self):
        say("MARK")


class Service:
    def __init__(self, mode, init):
        self.mode = mode
        self.bus = dbus.SessionBus() if mode == "notifd" else dbus.SystemBus()
        self.flags = {}
        # Replies of the calls "hold" keeps open.
        self.held_calls = []
        self.agent = None
        self.forger = None
        # A "begin" command sent when the agent unregisters, before that call is answered.
        self.begin_on_unregister = None
        # "hold-unregister": the next UnregisterAuthenticationAgent waits for
        # "release-unregister".
        self.hold_unregister = False
        self.held = None
        # "hold-get": the next Properties.Get of that property waits for "release-get".
        self.hold_get = None
        self.held_get = None
        # The connection owning polkitd's name: BeginAuthentication comes from it.
        self.polkitd = self.bus
        self.notifd = Notifd(self.bus, self) if mode == "notifd" else None
        self.marker = Marker(self.bus)
        self.objs = {}
        self.display = init.get("display", "")
        self.root = None
        self.manager = None
        if mode == "upower":
            self.root = UPowerRoot(self.bus, "/org/freedesktop/UPower",
                                   {"org.freedesktop.UPower": init.get("root", {})}, self)
        if mode == "polkitd":
            self.login = Login(self.bus)
            self.authority = Authority(self.bus, self)
        if mode == "bluez":
            self.manager = Manager(self.bus, self)
            self.agent_manager = AgentManager(self.bus, self)
        if mode == "nm":
            self.manager = Manager(self.bus, self, "/org/freedesktop")
        for path, ifaces in init.get("objects", {}).items():
            self.objs[path] = Obj(self.bus, path, ifaces, self)
        names = {"upower": ["org.freedesktop.UPower"],
                 "ppd": ["org.freedesktop.UPower.PowerProfiles", "net.hadess.PowerProfiles"],
                 "ppd-legacy": ["net.hadess.PowerProfiles"],
                 "bluez": ["org.bluez"],
                 "nm": ["org.freedesktop.NetworkManager"],
                 "notifd": [NOTIF],
                 # logind first: the agent asks it for its session once polkitd appears.
                 "polkitd": [LOGIN, POLKIT]}[mode]
        # polkitd's name may be taken over ("replace-polkitd"), as `polkitd --replace` does.
        self.names = [dbus.service.BusName(n, self.bus, allow_replacement=(n == POLKIT))
                      for n in names]

    def hold(self, flag, *what):
        """With `flag` set, a call is never answered (a daemon that hangs): prints HELD and
        keeps the call's reply callback, last argument of `what`, so the call stays open."""
        if not self.flags.get(flag):
            return False
        *what, ok = what
        self.held_calls.append(ok)
        say("HELD", *what)
        return True

    def obj(self, path):
        if self.root is not None and path == self.root.path:
            return self.root
        return self.objs[path]

    def add(self, path, ifaces):
        if path in self.objs:
            o = self.objs[path]
            for i, p in ifaces.items():
                o.ifaces[i] = typed_props(i, p)
        else:
            o = self.objs[path] = Obj(self.bus, path, ifaces, self)
        if self.mode == "upower":
            self.root.DeviceAdded(dbus.ObjectPath(path))
        elif self.manager:
            self.manager.InterfacesAdded(dbus.ObjectPath(path), dbus.Dictionary(
                {i: o.ifaces[i] for i in ifaces}, signature="sa{sv}"))

    def remove(self, path, ifaces):
        o = self.objs.get(path)
        if o is None:
            return
        gone = list(o.ifaces) if not ifaces else ifaces
        for i in gone:
            o.ifaces.pop(i, None)
        if not o.ifaces:
            del self.objs[path]
            o.remove_from_connection()
        if self.mode == "upower":
            self.root.DeviceRemoved(dbus.ObjectPath(path))
        elif self.manager:
            self.manager.InterfacesRemoved(dbus.ObjectPath(path), dbus.Array(gone, signature="s"))

    def call_agent(self, c, done=lambda: None):
        """BeginAuthentication or CancelAuthentication on the agent; `done` once answered."""
        # begin-forged: BeginAuthentication from a connection that does not own polkitd's name.
        op = c["op"]
        cookie = c["cookie"]
        sender, path = self.agent
        if op == "begin-forged" and self.forger is None:
            self.forger = dbus.SystemBus(private=True)
        bus = self.forger if op == "begin-forged" else self.polkitd
        tag = "CANCEL" if op == "cancel" else "BEGIN"
        if op == "cancel":
            member, sig, args = "CancelAuthentication", "s", [cookie]
        else:
            ids = [dbus.Struct((k, dbus.Dictionary({n: dbus.UInt32(v)}, signature="sv")))
                   for k, n, v in c.get("ids", [["unix-group", "gid", 0]])]
            member, sig = "BeginAuthentication", "sssa{ss}sa(sa{sv})"
            args = [c.get("action", "org.example.act"), c.get("message", "Authenticate"),
                    "dialog-password", dbus.Dictionary({}, signature="ss"), cookie,
                    dbus.Array(ids, signature="(sa{sv})")]

        def reply(*r):
            say(tag + "-REPLY", cookie)
            done()

        def error(e):
            say(tag + "-ERROR", cookie, e.get_dbus_name())
            done()
        bus.call_async(sender, path, POLKIT_AGENT, member, sig, args,
                       reply_handler=reply, error_handler=error, timeout=120)

    def command(self, c):
        op = c["op"]
        if op == "set":
            self.obj(c["path"]).update(c["iface"], c["props"])
        elif op == "invalidate":
            self.obj(c["path"]).update(c["iface"], c["props"], invalidate=True)
        elif op == "add":
            self.add(c["path"], c["ifaces"])
        elif op == "remove":
            self.remove(c["path"], c.get("ifaces"))
        elif op == "flag":
            self.flags[c["name"]] = c["value"]
        elif op in ("agent", "agent-forged"):
            # agent-forged: the same call from a connection that does not own org.bluez.
            m = c["method"]
            sender, path = self.agent
            if op == "agent-forged" and self.forger is None:
                self.forger = dbus.SystemBus(private=True)
            bus = self.bus if op == "agent" else self.forger
            bus.call_async(sender, path, "org.bluez.Agent1", m, AGENT_SIGS[m],
                                c.get("args", []),
                                reply_handler=lambda *r: say("AGENT-REPLY", m, *r),
                                error_handler=lambda e: say("AGENT-ERROR", m, e.get_dbus_name()),
                                timeout=120)
        elif op in ("begin", "begin-forged", "cancel"):
            self.call_agent(c)
        elif op == "begin-on-unregister":
            self.begin_on_unregister = dict(c, op="begin")
        elif op == "fail-held":
            # Answers every call "hold" kept open with org.bluez.Error.Failed.
            held, self.held_calls = self.held_calls, []
            for err in held:
                err(dbus.exceptions.DBusException("org.bluez.Error.Failed"))
        elif op == "hold-unregister":
            self.hold_unregister = True
        elif op == "hold-get":
            self.hold_get = c["prop"]
        elif op == "release-get":
            held, self.held_get = self.held_get, None
            held()
        elif op == "release-unregister":
            held, self.held = self.held, None
            held()
        elif op == "replace-polkitd":
            # A second polkitd takes the name over directly: one NameOwnerChanged with both the
            # old and the new owner set. Its registrations print as "POLKIT-NEXT".
            self.polkitd = dbus.SystemBus(private=True)
            self.successor = Authority(self.polkitd, self, "POLKIT-NEXT")
            self.names.append(dbus.service.BusName(POLKIT, self.polkitd, replace_existing=True))
        elif op == "watch-removematch":
            # Print every RemoveMatch on the bus, the agent's as "agent": a monitor (dbus-daemon's
            # BecomeMonitor) sees calls to the bus driver no other client can.
            self.monitor = dbus.SystemBus(private=True)
            self.monitor.call_blocking(DBUS, "/org/freedesktop/DBus", DBUS + ".Monitoring",
                                       "BecomeMonitor", "asu",
                                       ([f"type='method_call',interface='{DBUS}',"
                                         "member='RemoveMatch'"], 0))

            def seen(conn, m):
                if m.get_member() == "RemoveMatch":
                    who = "agent" if self.agent and m.get_sender() == self.agent[0] else "other"
                    say("REMOVEMATCH", who, *m.get_args_list())
                return dbus.lowlevel.HANDLER_RESULT_HANDLED
            self.monitor.add_message_filter(seen)
        elif op == "invoke":
            self.notifd.ActionInvoked(dbus.UInt32(c["id"]), c["key"])
        elif op == "dismiss":
            self.notifd.NotificationClosed(dbus.UInt32(c["id"]), dbus.UInt32(2))
        elif op == "quit":
            loop.quit()
        say("DONE", op)


def on_stdin(src, cond):
    line = sys.stdin.readline()
    if not line:
        loop.quit()
        return False
    if line.strip():
        svc.command(json.loads(line))
    return True


dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
loop = GLib.MainLoop()
svc = Service(sys.argv[1], json.loads(sys.argv[2]) if len(sys.argv) > 2 else {})
GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP, on_stdin)
say("READY")
loop.run()
