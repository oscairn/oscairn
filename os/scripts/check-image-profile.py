#!/usr/bin/env python3
"""Checks that an image and its device profile agree on the optional hardware (RFC 0035).

A profile that lacks a feature (no screen, Bluetooth, Wi-Fi, touch, battery, Ethernet, USB
device or host mode, camera) must leave nothing of it in the image: no Oscairn option, no
Buildroot package, no kernel option, no file, and no program or library naming its libraries.
A profile that declares a feature must have the Oscairn option and the kernel support for it.
Accelerators (RFC 0004) follow the same rule: each one's service is a feature of its own, and the
simulated accelerator, for tests only, is never in an image nor declared by a product's profile.

    check-image-profile.py --br2-config FILE --profile FILE [--kernel-config FILE] [--target DIR]
    check-image-profile.py --defconfigs DIR --profiles DIR

The first form checks one built image (board/common/post-build.sh runs it, so a mismatch fails the
build); the second checks every product defconfig in DIR against the profile its overlay starts
the app with, from the Buildroot options alone (no build needed). Exits 1 on any mismatch.
"""
import argparse
import glob
import json
import os
import re
import sys
from dataclasses import dataclass, field


@dataclass
class Feature:
    name: str
    declared: object  # profile -> bool
    options: list = field(default_factory=list)  # Oscairn's Buildroot options: on exactly when declared
    packages: list = field(default_factory=list)  # Buildroot options: off when not declared
    kernel_on: list = field(default_factory=list)  # kernel options: set when declared
    kernel_off: list = field(default_factory=list)  # kernel options (a trailing * for a prefix): unset when not
    files: list = field(default_factory=list)  # globs in the target: absent when not declared
    libraries: list = field(default_factory=list)  # names no file in the target may contain when not declared
    test_only: bool = False  # never in a product: declaring it is itself a mismatch


def screens(p):
    return p.get("screens") or []


def accelerators(p):
    return (p.get("hardware") or {}).get("accelerators") or []


FEATURES = [
    Feature("display", lambda p: bool(screens(p)),
            options=["BR2_PACKAGE_OSCAIRN_STANDALONE"],
            packages=["BR2_PACKAGE_XORG7", "BR2_PACKAGE_XSERVER_XORG_SERVER"],
            kernel_on=["CONFIG_DRM", "CONFIG_DRM_FBDEV_EMULATION", "CONFIG_FB_DEVICE"],
            kernel_off=["CONFIG_DRM", "CONFIG_FB"],
            files=["usr/bin/Oscairn", "usr/bin/Xorg", "usr/lib/libX11.so*", "usr/share/oscairn/themes"],
            libraries=["libX11.so"]),
    Feature("touch", lambda p: any(s.get("touch") for s in screens(p)),
            kernel_on=["CONFIG_INPUT_TOUCHSCREEN"], kernel_off=["CONFIG_INPUT_TOUCHSCREEN"]),
    Feature("bluetooth", lambda p: bool(p.get("bluetooth")),
            options=["BR2_PACKAGE_OSCAIRN_BLUETOOTH"],
            packages=["BR2_PACKAGE_BLUEZ5_UTILS", "BR2_PACKAGE_BLUEZ_ALSA", "BR2_PACKAGE_BLUEZ_TOOLS"],
            kernel_on=["CONFIG_BT"], kernel_off=["CONFIG_BT"],
            files=["usr/libexec/bluetooth", "usr/bin/bluealsa*", "usr/lib/libbluetooth.so*",
                   "usr/lib/alsa-lib/*bluealsa*", "lib/firmware/brcm/*.hcd", "lib/firmware/rtl_bt"],
            libraries=["libbluetooth.so"]),
    Feature("wifi", lambda p: bool((p.get("network") or {}).get("wifi")),
            packages=["BR2_PACKAGE_WPA_SUPPLICANT", "BR2_PACKAGE_IWD", "BR2_PACKAGE_HOSTAPD",
                      "BR2_PACKAGE_IW", "BR2_PACKAGE_WIRELESS_TOOLS", "BR2_PACKAGE_WIRELESS_REGDB"],
            kernel_on=["CONFIG_CFG80211"], kernel_off=["CONFIG_CFG80211", "CONFIG_MAC80211"],
            files=["usr/sbin/wpa_supplicant", "usr/libexec/iwd", "usr/sbin/iw", "lib/firmware/regulatory.db",
                   "lib/firmware/brcm/*.bin", "lib/firmware/iwlwifi-*", "lib/firmware/ath*k", "lib/firmware/rtw8*"]),
    Feature("ethernet", lambda p: bool((p.get("network") or {}).get("ethernet")),
            kernel_on=["CONFIG_NETDEVICES"], kernel_off=["CONFIG_ETHERNET", "CONFIG_VIRTIO_NET"]),
    Feature("battery", lambda p: bool((p.get("power") or {}).get("battery")),
            kernel_on=["CONFIG_POWER_SUPPLY"], kernel_off=["CONFIG_BATTERY_*", "CONFIG_CHARGER_*"]),
    Feature("usb-gadget", lambda p: bool((p.get("midi") or {}).get("usbDevice")),
            options=["BR2_PACKAGE_OSCAIRN_USB_GADGET"],
            kernel_on=["CONFIG_USB_GADGET", "CONFIG_USB_CONFIGFS_F_MIDI", "CONFIG_USB_CONFIGFS_F_UAC2"],
            kernel_off=["CONFIG_USB_GADGET"]),
    Feature("usb-host", lambda p: bool((p.get("midi") or {}).get("usbHost")),
            kernel_on=["CONFIG_USB", "CONFIG_SND_USB_AUDIO"], kernel_off=["CONFIG_USB"]),
    Feature("roland-panel", lambda p: (p.get("panel") or {}).get("protocol") == "roland-i2c-slave"
            or (p.get("keyboard") or {}).get("protocol") == "roland-bmc-stock",
            options=["BR2_PACKAGE_OSCAIRN_ROLAND_FANTOM0"],
            files=["usr/bin/oscairn-panel-dump", "usr/bin/oscairn-panel-holder", "usr/share/oscairn/panels",
                   "oscairn-fantom-start", "oscairn-fantom-stop"]),
    Feature("camera", lambda p: bool(p.get("camera")),
            kernel_on=["CONFIG_MEDIA_SUPPORT"], kernel_off=["CONFIG_MEDIA_SUPPORT"],
            packages=["BR2_PACKAGE_LIBCAMERA", "BR2_PACKAGE_LIBV4L"]),
    # RFC 0004: an accelerator's service is compiled in only for a profile declaring that
    # accelerator. Each real one adds its feature here (its BR2_PACKAGE_OSCAIRN_HARDWARE_ option,
    # its files, and its service ABI as a name no binary may hold without it) as it lands; the
    # simulated one's code names its ABI, oscairn.sim/1.
    Feature("simulated-accelerator", lambda p: any(a.get("kind") == "simulated" for a in accelerators(p)),
            libraries=["oscairn.sim/"], test_only=True),
]


# Oscairn's options that only set up the image's own kernel. An image without one (a root
# filesystem for a chroot on another system's kernel) cannot set them: that kernel decides.
KERNEL_ONLY_OPTIONS = {"BR2_PACKAGE_OSCAIRN_USB_GADGET"}


def read_config(path):
    """A Kconfig file's set options, as {name: value} (unset and commented options left out)."""
    values = {}
    for line in open(path):
        m = re.match(r"^(\w+)=(.*)$", line.strip())
        if m and m.group(2) != "n":
            values[m.group(1)] = m.group(2)
    return values


def kernel_matches(config, pattern):
    if pattern.endswith("*"):
        return sorted(k for k in config if k.startswith(pattern[:-1]))
    return [pattern] if pattern in config else []


def profile_of_overlays(overlays):
    """The profile id the session starts the app with: --profile in the last overlay's
    etc/default/oscairn."""
    found = None
    for overlay in overlays:
        path = os.path.join(overlay, "etc/default/oscairn")
        if os.path.exists(path):
            m = re.search(r"--profile\s+([\w.-]+)", open(path).read())
            found = m.group(1) if m else None
    return found


def check(name, br2, profile, kernel=None, target=None):
    """Mismatches between one image (its Buildroot options, kernel config, target folder) and
    its profile, as messages."""
    problems = []
    def problem(feature, text):
        problems.append(f"{name}: {feature.name} {text}")
    for f in FEATURES:
        declared = f.declared(profile)
        if declared and f.test_only:
            problem(f, "is for tests only, and no product's profile declares it")
        for option in f.options:
            if declared and option not in br2 and (option not in KERNEL_ONLY_OPTIONS or "BR2_LINUX_KERNEL" in br2):
                problem(f, f"declared by the profile, but {option} is not set")
            if not declared and option in br2:
                problem(f, f"not in the profile, but {option} is set")
        if not declared:
            for option in f.packages:
                if option in br2:
                    problem(f, f"not in the profile, but {option} is set")
        if kernel is not None:
            if declared:
                for option in f.kernel_on:
                    if option not in kernel:
                        problem(f, f"declared by the profile, but the kernel lacks {option}")
            else:
                for pattern in f.kernel_off:
                    for option in kernel_matches(kernel, pattern):
                        problem(f, f"not in the profile, but the kernel has {option}={kernel[option]}")
        if target is not None and not declared:
            for pattern in f.files:
                for path in sorted(glob.glob(os.path.join(target, pattern))):
                    problem(f, f"not in the profile, but the image has /{os.path.relpath(path, target)}")
            if f.libraries:
                for path in elf_files(target):
                    data = open(path, "rb").read()
                    for library in f.libraries:
                        if library.encode() in data:
                            problem(f, f"not in the profile, but /{os.path.relpath(path, target)} names {library}")
    return problems


def elf_files(target):
    for root, _, files in os.walk(target):
        for name in files:
            path = os.path.join(root, name)
            if os.path.isfile(path) and not os.path.islink(path):
                with open(path, "rb") as f:
                    if f.read(4) == b"\x7fELF":
                        yield path


def check_defconfigs(defconfigs, profiles, external):
    """Every product defconfig against its profile, from the Buildroot options alone."""
    problems = []
    checked = []
    for path in sorted(glob.glob(os.path.join(defconfigs, "*_defconfig"))):
        br2 = read_config(path)
        overlays = [o.replace("$(BR2_EXTERNAL_OSCAIRN_PATH)", external)
                    for o in br2.get("BR2_ROOTFS_OVERLAY", "").strip('"').split()]
        name = os.path.basename(path)
        if "BR2_PACKAGE_OSCAIRN" not in br2:
            continue
        pid = profile_of_overlays(overlays)
        if not pid:
            problems.append(f"{name}: no --profile in its overlays' etc/default/oscairn")
            continue
        profile_path = os.path.join(profiles, pid + ".json")
        if not os.path.exists(profile_path):
            problems.append(f"{name}: its profile {pid} is not in {profiles}")
            continue
        checked.append(f"{name} ({pid})")
        problems += check(name, br2, json.load(open(profile_path)))
    return checked, problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--br2-config")
    parser.add_argument("--profile")
    parser.add_argument("--kernel-config")
    parser.add_argument("--target")
    parser.add_argument("--defconfigs")
    parser.add_argument("--profiles")
    args = parser.parse_args()
    if args.defconfigs:
        external = os.path.dirname(os.path.abspath(args.defconfigs.rstrip("/")))
        checked, problems = check_defconfigs(args.defconfigs, args.profiles, external)
        print("checked:", ", ".join(checked) or "nothing")
    else:
        profile = json.load(open(args.profile))
        kernel = read_config(args.kernel_config) if args.kernel_config else None
        problems = check(profile.get("id", args.profile), read_config(args.br2_config), profile, kernel, args.target)
        declared = [f.name for f in FEATURES if f.declared(profile)]
        print(f"profile {profile.get('id')}: has {', '.join(declared) or 'none of the optional hardware'}")
    for p in problems:
        print("MISMATCH:", p)
    print("FAIL" if problems else "PASS: the image and its profile agree")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
