#!/usr/bin/env python3
"""Tests check-image-profile.py: every product defconfig agrees with its profile, and a feature
left out of a profile fails the check wherever anything of it is left in an image (a Buildroot
option or package, a kernel option, a file, a program naming its library), as does a feature a
profile declares that the image lacks."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("check", here / "check-image-profile.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)
failures = []


def expect(condition, text):
    print(("ok: " if condition else "FAIL: ") + text)
    if not condition:
        failures.append(text)


# The real defconfigs and profiles, as the build would pair them.
result = subprocess.run([sys.executable, str(here / "check-image-profile.py"),
                         "--defconfigs", str(here.parent / "buildroot/configs"),
                         "--profiles", str(here.parents[1] / "hosts/standalone/profiles")],
                        capture_output=True, text=True)
print(result.stdout, end="")
expect(result.returncode == 0 and "oscairn_qemu_aarch64_defconfig (qemu-aarch64)" in result.stdout
       and "oscairn_imx8mm_evk_defconfig (imx8mm-evk)" in result.stdout
       and "oscairn_fantom_bundle_defconfig (fantom-07)" in result.stdout,
       "every product defconfig agrees with the profile its overlay names")

# Every profile present, each feature read from it without an error.
for path in sorted((here.parents[1] / "hosts/standalone/profiles").glob("*.json")):
    profile = json.loads(path.read_text())
    [f.declared(profile) for f in check.FEATURES]

# A pedal: no screen, no network, nothing optional.
pedal = {"id": "pedal", "audio": {"outputs": 2}}
base = {"BR2_PACKAGE_OSCAIRN": "y"}
kernel = {"CONFIG_SND": "y", "CONFIG_POWER_SUPPLY": "y"}
with tempfile.TemporaryDirectory(prefix="oscairn-image-") as temporary:
    target = Path(temporary)
    (target / "usr/bin").mkdir(parents=True)
    (target / "usr/bin/oscairn-engine").write_bytes(b"\x7fELF\x02\x01\x01 libasound.so.2\0")
    expect(check.check("pedal", base, pedal, kernel, str(target)) == [], "a pedal with nothing optional passes")

    def fails(what, br2=None, kernel_extra=None, files=(), profile=pedal, needle=""):
        with tempfile.TemporaryDirectory(prefix="oscairn-image-") as case:
            for name, data in files:
                (Path(case) / name).parent.mkdir(parents=True, exist_ok=True)
                (Path(case) / name).write_bytes(data)
            problems = check.check("pedal", {**base, **(br2 or {})}, profile, {**kernel, **(kernel_extra or {})}, case)
        expect(len(problems) == 1 and needle in problems[0], f"{what} fails ({problems})")

    fails("the app without a screen", br2={"BR2_PACKAGE_OSCAIRN_STANDALONE": "y"}, needle="BR2_PACKAGE_OSCAIRN_STANDALONE")
    fails("X without a screen", br2={"BR2_PACKAGE_XORG7": "y"}, needle="BR2_PACKAGE_XORG7")
    fails("DRM without a screen", kernel_extra={"CONFIG_DRM": "m"}, needle="CONFIG_DRM=m")
    fails("libX11 left in the image", files=[("usr/lib/libX11.so.6", b"")], needle="/usr/lib/libX11.so.6")
    fails("a program linked to libX11", files=[("usr/bin/tool", b"\x7fELF libX11.so.6\0")], needle="names libX11.so")
    fails("Bluetooth in the kernel", kernel_extra={"CONFIG_BT": "y"}, needle="CONFIG_BT=y")
    fails("BlueZ in the image", br2={"BR2_PACKAGE_BLUEZ5_UTILS": "y"}, needle="BR2_PACKAGE_BLUEZ5_UTILS")
    fails("bluetoothd left behind", files=[("usr/libexec/bluetooth/bluetoothd", b"")], needle="bluetooth")
    fails("a program linked to libbluetooth", files=[("usr/lib/libx.so", b"\x7fELF libbluetooth.so.3\0")], needle="libbluetooth.so")
    fails("Wi-Fi in the kernel", kernel_extra={"CONFIG_CFG80211": "m"}, needle="CONFIG_CFG80211")
    fails("wpa_supplicant in the image", br2={"BR2_PACKAGE_WPA_SUPPLICANT": "y"}, needle="WPA_SUPPLICANT")
    fails("Wi-Fi firmware left behind", files=[("lib/firmware/brcm/brcmfmac43455-sdio.bin", b"")], needle="brcmfmac")
    fails("a touch screen driver", kernel_extra={"CONFIG_INPUT_TOUCHSCREEN": "y"}, needle="TOUCHSCREEN")
    fails("a battery driver", kernel_extra={"CONFIG_BATTERY_BQ27XXX": "m"}, needle="CONFIG_BATTERY_BQ27XXX")
    fails("a charger driver", kernel_extra={"CONFIG_CHARGER_GPIO": "y"}, needle="CONFIG_CHARGER_GPIO")
    fails("Ethernet without network.ethernet", kernel_extra={"CONFIG_ETHERNET": "y"}, needle="CONFIG_ETHERNET")
    fails("the USB gadget option without midi.usbDevice", br2={"BR2_PACKAGE_OSCAIRN_USB_GADGET": "y"}, needle="USB_GADGET")
    fails("the USB gadget in the kernel without midi.usbDevice", kernel_extra={"CONFIG_USB_GADGET": "y"}, needle="CONFIG_USB_GADGET")
    fails("a USB host without midi.usbHost", kernel_extra={"CONFIG_USB": "y"}, needle="CONFIG_USB=")
    fails("a camera stack without a camera", kernel_extra={"CONFIG_MEDIA_SUPPORT": "y"}, needle="MEDIA_SUPPORT")

    fails("the Roland panel option without the panel", br2={"BR2_PACKAGE_OSCAIRN_ROLAND_FANTOM0": "y"}, needle="ROLAND_FANTOM0")
    fails("the panel dump tool left behind", files=[("usr/bin/oscairn-panel-dump", b"")], needle="oscairn-panel-dump")
    fails("the simulated accelerator's service in a program", files=[("usr/bin/oscairn-engine", b"\x7fELF oscairn.sim/1\0")],
          needle="names oscairn.sim/")

    # Declared but missing.
    fails("a FANTOM panel without the reader", profile={**pedal, "panel": {"protocol": "roland-i2c-slave"}},
          needle="BR2_PACKAGE_OSCAIRN_ROLAND_FANTOM0 is not set")
    fails("FANTOM keys without the reader", profile={**pedal, "keyboard": {"protocol": "roland-bmc-stock", "device": "/dev/ttyGS0"}},
          needle="BR2_PACKAGE_OSCAIRN_ROLAND_FANTOM0 is not set")
    fails("a screen without the app", profile={**pedal, "screens": [{"width": 800, "height": 480, "surface": "panel", "layout": "x"}]},
          kernel_extra={"CONFIG_DRM": "y", "CONFIG_DRM_FBDEV_EMULATION": "y", "CONFIG_FB_DEVICE": "y"}, needle="BR2_PACKAGE_OSCAIRN_STANDALONE is not set")
    fails("a touch screen without touch input", profile={**pedal, "screens": [{"width": 800, "height": 480, "touch": True, "surface": "panel", "layout": "x"}]},
          br2={"BR2_PACKAGE_OSCAIRN_STANDALONE": "y"}, kernel_extra={"CONFIG_DRM": "y", "CONFIG_DRM_FBDEV_EMULATION": "y", "CONFIG_FB_DEVICE": "y"},
          needle="lacks CONFIG_INPUT_TOUCHSCREEN")
    fails("Bluetooth declared, not built", profile={**pedal, "bluetooth": {"midi": True}}, kernel_extra={"CONFIG_BT": "y"},
          needle="BR2_PACKAGE_OSCAIRN_BLUETOOTH is not set")
    fails("Wi-Fi declared, not in the kernel", profile={**pedal, "network": {"wifi": True}}, needle="lacks CONFIG_CFG80211")
    fails("a product profile declaring the simulated accelerator", needle="for tests only",
          profile={**pedal, "hardware": {"accelerators": [{"id": "sim0", "kind": "simulated", "abi": "oscairn.sim/1",
                                                           "operations": ["oscairn.sim.sine/1"]}]}})
    fails("a USB device port without the gadget option", br2={"BR2_LINUX_KERNEL": "y"}, profile={**pedal, "midi": {"usbDevice": True}},
          kernel_extra={"CONFIG_USB_GADGET": "y", "CONFIG_USB_CONFIGFS_F_MIDI": "y", "CONFIG_USB_CONFIGFS_F_UAC2": "y"},
          needle="BR2_PACKAGE_OSCAIRN_USB_GADGET is not set")
    expect(check.check("pedal", base, {**pedal, "midi": {"usbDevice": True}}) == [],
           "an image without a kernel of its own (a chroot bundle) needs no gadget option for a USB device port")

# The command line fails a mismatched image (as post-build.sh runs it, failing the build).
with tempfile.TemporaryDirectory(prefix="oscairn-image-") as temporary:
    t = Path(temporary)
    (t / ".config").write_text("BR2_PACKAGE_OSCAIRN=y\nBR2_PACKAGE_OSCAIRN_STANDALONE=y\n")
    (t / "pedal.json").write_text(json.dumps(pedal))
    r = subprocess.run([sys.executable, str(here / "check-image-profile.py"), "--br2-config", str(t / ".config"),
                        "--profile", str(t / "pedal.json")], capture_output=True, text=True)
    expect(r.returncode == 1 and "MISMATCH" in r.stdout, "the command exits 1 on a mismatch")

print("FAIL" if failures else "PASS")
sys.exit(1 if failures else 0)
