# 06 — Wi-Fi, Bluetooth and time hardening

Status: open · Issues: #24 (Wi-Fi reliability), #7 (hidden SSID device
check), #5 (Bluetooth pairing, address, UART speed), #8 (time sync device
check) · Depends on: 00 · Feeds: 03 (BT audio), 14

## Goal

The radios that already "work" become dependable for daily use. They
reconnect on their own, recover from failures, pair real accessories, and
keep correct time.

## Deliverables

### Wi-Fi (#24, #7)

1. **Baseline** on the current image:
   - driver and firmware manifest verified;
   - `wlan0` scans without manual `scansuppress` changes;
   - the startup order (radio → NetworkManager → wpa_supplicant →
     `ferrix-networkd`).
2. **Security matrix** on owner-controlled test APs: WPA2-PSK, WPA2/WPA3
   transition (confirm the negotiated AKM is SAE), and SAE-only with PMF
   required. For each: join, DHCP, default route, DNS, reachability.
3. **Failure and recovery:**
   - cold boot autoconnects to a saved profile;
   - radio off/on;
   - NetworkManager / wpa_supplicant restart;
   - AP loss and return;
   - roaming between two APs with the same SSID;
   - resume from suspend (spec 04);
   - sustained connection of at least 2 h with throughput samples.
4. **Bands:** record which 2.4/5/6 GHz channels the device actually uses
   under the local regulatory domain. Check how the regulatory domain is
   set (country code source).
5. **Persistence:** Wi-Fi profiles survive a reflash, or there is a
   documented restore step. Today they're lost on every userdata reflash.
6. **#7:** the hidden-SSID join and wrong-password flow on the device
   (also listed in spec 00).

### Bluetooth (#5)

7. **Pairing with real devices,** with framebuffer captures:
   - headphones (Just Works / confirm);
   - a keyboard (passkey);
   - a phone (numeric comparison);
   then reconnect after a reboot, disconnect, and Forget. Map any
   unexpected BlueZ errors into the adapter's typed failures.
8. **BD address:** find the factory address source exposed read-only on
   Linux (cmdline, device tree, persist), and set it before power-on. If it
   can't be done, document why. Never commit the address.
9. **UART speed:** switch from 115200 to the BCM operating speed (3–4 Mbaud)
   through btattach / hci_bcm `max-speed`. Measure the time from boot to
   hci0 before and after; it is about 90 s today.
10. **Android Bluetooth:** decide whether Android apps get Bluetooth.
    Currently Ferrix owns the controller and Android sees none. If they
    should, design a mediated route; don't pass the controller through to
    Android.
11. Update the stale `[bluetooth]` evidence string ("pairing not
    exercised").

### Time (#8)

12. The chrony device checks from spec 00. Then a time zone source: a
    Settings picker, with an optional network time zone later. Today it is
    fixed to UTC.

## Acceptance

- Every #24 test case recorded, with failures attributed to a layer (driver,
  supplicant, NetworkManager, Ferrix).
- #7, #5 and #8 acceptance met on the exact device.
- No SSIDs, passwords, MACs or BD addresses in Git or the evidence.

## Owner-gated

Test APs and accessories (the owner provides them). Nothing else is gated.
