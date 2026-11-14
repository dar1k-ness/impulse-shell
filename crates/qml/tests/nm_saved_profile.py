"""python-dbusmock's NetworkManager template, started with one saved Wi-Fi profile.

NetworkManager reads its saved profiles before it takes its bus name, so a client that asks
right after the name appears finds them; the profile is added before dbusmock's main loop
serves any call. Parameters: "uuid" and "ssid" of the profile.
"""

from dbusmock.templates.networkmanager import *  # noqa: F401,F403
from dbusmock.templates import networkmanager as _nm


def load(mock, parameters):
    _nm.load(mock, parameters)
    settings = {
        "connection": {
            "id": parameters["ssid"],
            "uuid": parameters["uuid"],
            "type": "802-11-wireless",
        },
        "802-11-wireless": {"ssid": dbus.ByteArray(parameters["ssid"].encode())},
    }
    dbusmock.get_object(_nm.SETTINGS_OBJ).SettingsAddConnection(settings)
