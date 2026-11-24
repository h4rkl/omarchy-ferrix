# 13 — Updates, rollback and user-data recovery

Status: open, not started · ADR:
[012](../../docs/adr/012-updates-rollback-and-data-layout.md) (proposed)
· Issue: #30 · Depends on: 00, 12 (encryption layout) · Feeds: 14

## Goal

A user can update Ferrix, survive a failed update, and recover their data,
without a USB cable or a developer. Every outcome is reported truthfully.
Today there are only dev flashes (which reset userdata) and the stock
factory restore (which wipes everything).

## Deliverables

1. **Layout document.**
   - The exact current partitions on the dev route: boot, vendor_boot and
     vbmeta on slot b, and the ext4 rootfs in userdata.
   - What each flash or restore destroys.
   - The coupling between kernel, firmware, rootfs and the Waydroid image.
   Don't promise A/B rollback while the rootfs shares userdata.
2. **Release layout design.** Separate the immutable system (rootfs + kernel,
   A/B or image-based) from persistent user data. Options:
   - A/B rootfs images in userdata, with a boot-counting fallback;
   - pmOS package upgrades with snapshots (btrfs or similar);
   - a slot-based route using the boot slots for kernel images.
   Pick one in an ADR, checked against the recovery invariant (writes only
   to partitions the factory image rewrites).
3. **Signed or hash-verified releases.**
   - A release manifest of pinned inputs and hashes; already partly there
     in the dev-compose manifests.
   - Signature verification before applying an update.
   - No unpinned firmware is ever fetched or applied.
4. **Health check and rollback.** A bounded first-boot health check (shell
   up, appd up, network adapter alive). Failure falls back to the previous
   system automatically, and a boot failure is never reported as success.
5. **Migrations.** Versioned schema migrations for Ferrix state (contacts,
   the SMS store, grants, settings, NetworkManager profiles), tested
   forward and with interruption.
6. **Backup and restore of user data.** An explicit export/import of home,
   app data and Waydroid data to external storage or a host, encrypted
   (spec 12). Document which paths need a wipe.
7. **Update UX.** Settings > System update: check, download, verify, apply
   on reboot, and the result. Run it from a local or host-served channel
   first.
8. **Cadence and ownership.** A written policy for kernel, firmware,
   postmarketOS package and LineageOS/Waydroid image updates, with a
   security-response owner and pin-bump procedure.

## Acceptance

On the exact device, with test data:
- one version-to-version upgrade;
- one interrupted upgrade;
- one rollback;
- one data migration.
Record which paths preserve data and which wipe it. A documented recovery
path for ordinary users exists. Stock restore remains available and tested.

## Owner-gated

The release layout ADR, signing key ownership, and any change to which
partitions a workflow writes.
