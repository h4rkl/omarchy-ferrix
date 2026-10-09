//! Static, libc-free `reboot(LINUX_REBOOT_CMD_RESTART2, "bootloader")` for the
//! Pixel 9a bring-up initramfs (tools/pixel-9a/dev-compose.py). busybox
//! `reboot` cannot pass a restart command, and the init's checkpoints must be
//! able to return the device to bootloader Fastboot before any library,
//! driver, or filesystem beyond the initramfs is available.
#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_SYNC: usize = 81;
const SYS_EXIT: usize = 93;
const SYS_REBOOT: usize = 142;
const LINUX_REBOOT_MAGIC1: usize = 0xfee1_dead;
const LINUX_REBOOT_MAGIC2: usize = 672_274_793;
const LINUX_REBOOT_CMD_RESTART2: usize = 0xa1b2_c3d4;

unsafe fn syscall4(number: usize, a: usize, b: usize, c: usize, d: usize) -> isize {
    let result: isize;
    asm!(
        "svc 0",
        in("x8") number,
        inlateout("x0") a as isize => result,
        in("x1") b,
        in("x2") c,
        in("x3") d,
        options(nostack),
    );
    result
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        syscall4(SYS_SYNC, 0, 0, 0, 0);
        syscall4(
            SYS_REBOOT,
            LINUX_REBOOT_MAGIC1,
            LINUX_REBOOT_MAGIC2,
            LINUX_REBOOT_CMD_RESTART2,
            b"bootloader\0".as_ptr() as usize,
        );
        syscall4(SYS_EXIT, 1, 0, 0, 0);
    }
    loop {}
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
