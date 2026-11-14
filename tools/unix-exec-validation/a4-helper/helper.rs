#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

// Fixed x86_64 Linux image. No libc, allocator, TLS or runtime initialization.
const LIFE: u64 = 15_000_000_000;
#[cfg(not(test))]
const ARGS: u8 = 64;
const CLOCK: u8 = 65;
const EXPIRED: u8 = 66;
const IO: u8 = 67;
const PROTOCOL: u8 = 68;
const READY: u8 = b'R';
const RELEASE: u8 = b'G';
const EINTR: i64 = -4;

fn decimal(bytes: &[u8], max: u64) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 19 || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    let mut n = 0u64;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add(u64::from(b - b'0'))?;
        if n > max {
            return None;
        }
    }
    Some(n)
}

fn arguments(args: &[&[u8]]) -> Option<(i32, i32, u64)> {
    if args.len() != 3 {
        return None;
    }
    let ready = decimal(args[0], i32::MAX as u64)? as i32;
    let release = decimal(args[1], i32::MAX as u64)? as i32;
    let deadline = decimal(args[2], i64::MAX as u64)?;
    if ready < 3 || release < 3 || ready == release || deadline == 0 {
        return None;
    }
    Some((ready, release, deadline))
}

fn timeout(deadline: u64, now: u64) -> Result<i32, u8> {
    let ns = deadline.saturating_sub(now);
    if ns == 0 {
        return Err(EXPIRED);
    }
    // Ceiling to milliseconds; never use poll's negative/infinite timeout.
    Ok(ns.div_ceil(1_000_000).min(i32::MAX as u64) as i32)
}

// Static dispatch only. Tests compile out the entire native module.
trait Ops {
    fn now(&mut self) -> Result<u64, u8>;
    fn poll(&mut self, fd: i32, events: i16, ms: i32) -> (i64, i16);
    fn write(&mut self, fd: i32, byte: &u8) -> i64;
    fn read(&mut self, fd: i32, byte: &mut u8) -> i64;
    fn close(&mut self, fd: i32) -> i64;
}

fn remaining(ops: &mut impl Ops, deadline: u64, last: &mut u64) -> Result<i32, u8> {
    let now = ops.now()?;
    if now < *last {
        return Err(CLOCK);
    }
    *last = now;
    timeout(deadline, now)
}

fn wait(ops: &mut impl Ops, fd: i32, events: i16, deadline: u64, last: &mut u64) -> Result<(), u8> {
    loop {
        let ms = remaining(ops, deadline, last)?;
        let (rc, revents) = ops.poll(fd, events, ms);
        if rc == EINTR {
            continue;
        }
        // Timeout must recheck the absolute clock (rounding/spurious return).
        if rc == 0 && revents == 0 {
            continue;
        }
        if rc != 1 || revents != events {
            return Err(IO);
        }
        remaining(ops, deadline, last)?;
        return Ok(());
    }
}

fn run(ops: &mut impl Ops, ready: i32, release: i32, deadline: u64) -> Result<(), u8> {
    let mut last = ops.now()?;
    if deadline <= last || deadline.saturating_sub(last) > LIFE {
        return Err(EXPIRED);
    }
    loop {
        wait(ops, ready, 4, deadline, &mut last)?; // POLLOUT
        match ops.write(ready, &READY) {
            EINTR => continue,
            1 => break,
            _ => return Err(IO),
        }
    }
    // Linux close must not retry EINTR: descriptor may already be closed.
    if ops.close(ready) != 0 {
        return Err(IO);
    }
    loop {
        wait(ops, release, 1, deadline, &mut last)?; // POLLIN
        let mut byte = 0;
        match ops.read(release, &mut byte) {
            EINTR => continue,
            1 if byte == RELEASE => break,
            1 => return Err(PROTOCOL),
            _ => return Err(IO), // EOF and all other errors
        }
    }
    if ops.close(release) != 0 {
        return Err(IO);
    }
    loop {
        let ms = remaining(ops, deadline, &mut last)?;
        let (rc, revents) = ops.poll(-1, 0, ms); // nfds=0; no descriptor access
        if (rc != 0 && rc != EINTR) || revents != 0 {
            return Err(IO);
        }
    }
}

#[cfg(not(test))]
mod native {
    use super::*;
    use core::arch::{asm, global_asm};

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    compile_error!("fixed helper requires x86_64 Linux");

    global_asm!(
        ".global _start",
        "_start:",
        "mov rdi, rsp",
        "and rsp, -16",
        "call entry",
        "ud2",
    );

    // x86_64 Linux syscall ABI; negative errno is returned directly, no TLS.
    unsafe fn syscall(n: i64, a: usize, b: usize, c: usize) -> i64 {
        let result: i64;
        unsafe {
            asm!("syscall", inlateout("rax") n => result,
                in("rdi") a, in("rsi") b, in("rdx") c,
                lateout("rcx") _, lateout("r11") _, options(nostack));
        }
        result
    }

    fn exit(code: u8) -> ! {
        unsafe {
            syscall(60, code as usize, 0, 0);
            asm!("ud2", options(noreturn));
        }
    }

    #[repr(C)]
    struct Timespec {
        sec: i64,
        ns: i64,
    }
    #[repr(C)]
    struct Pollfd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    struct Native;
    impl Ops for Native {
        fn now(&mut self) -> Result<u64, u8> {
            let mut ts = Timespec { sec: 0, ns: 0 };
            let rc = unsafe { syscall(228, 1, &mut ts as *mut _ as usize, 0) };
            if rc != 0 || ts.sec < 0 || !(0..1_000_000_000).contains(&ts.ns) {
                return Err(CLOCK);
            }
            (ts.sec as u64)
                .checked_mul(1_000_000_000)
                .and_then(|s| s.checked_add(ts.ns as u64))
                .ok_or(CLOCK)
        }
        fn poll(&mut self, fd: i32, events: i16, ms: i32) -> (i64, i16) {
            let mut p = Pollfd {
                fd,
                events,
                revents: 0,
            };
            let count = usize::from(fd >= 0);
            let rc = unsafe { syscall(7, &mut p as *mut _ as usize, count, ms as usize) };
            (rc, p.revents)
        }
        fn write(&mut self, fd: i32, byte: &u8) -> i64 {
            unsafe { syscall(1, fd as usize, byte as *const _ as usize, 1) }
        }
        fn read(&mut self, fd: i32, byte: &mut u8) -> i64 {
            unsafe { syscall(0, fd as usize, byte as *mut _ as usize, 1) }
        }
        fn close(&mut self, fd: i32) -> i64 {
            unsafe { syscall(3, fd as usize, 0, 0) }
        }
    }

    // Kernel-provided argv pointers; bounded scan, no environment access.
    unsafe fn arg<'a>(p: *const u8) -> Option<&'a [u8]> {
        for len in 0..=19 {
            if unsafe { *p.add(len) } == 0 {
                return Some(unsafe { core::slice::from_raw_parts(p, len) });
            }
        }
        None
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn entry(stack: *const usize) -> ! {
        if unsafe { *stack } != 4 {
            exit(ARGS);
        }
        let mut args: [&[u8]; 3] = [&[]; 3];
        for (i, dst) in args.iter_mut().enumerate() {
            *dst = match unsafe { arg(*stack.add(i + 2) as *const u8) } {
                Some(value) => value,
                None => exit(ARGS),
            };
        }
        let (ready, release, deadline) = match arguments(&args) {
            Some(value) => value,
            None => exit(ARGS),
        };
        match run(&mut Native, ready, release, deadline) {
            Err(code) => exit(code),
            Ok(()) => exit(PROTOCOL), // there is no success exit
        }
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        exit(69)
    }
}

#[cfg(test)]
mod tests;
