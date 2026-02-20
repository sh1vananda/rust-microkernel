use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::instructions::port::Port;

/// Monotonic uptime counter incremented by the PIT tick handler.
static UPTIME_MS: AtomicU64 = AtomicU64::new(0);

/// Increment the uptime counter. Called from the timer interrupt handler.
pub fn tick(ms: u64) {
    UPTIME_MS.fetch_add(ms, Ordering::Relaxed);
}

/// Get the monotonic uptime in milliseconds since boot.
pub fn uptime_ms() -> u64 {
    UPTIME_MS.load(Ordering::Relaxed)
}

/// Read the current time from the CMOS Real-Time Clock.
/// Returns a rough Unix-like timestamp (seconds since 2000-01-01 for simplicity).
pub fn unix_timestamp() -> u64 {
    let sec = read_cmos(0x00) as u64;
    let min = read_cmos(0x02) as u64;
    let hour = read_cmos(0x04) as u64;
    let day = read_cmos(0x07) as u64;
    let month = read_cmos(0x08) as u64;
    let year = read_cmos(0x09) as u64;

    // Convert BCD to binary (CMOS default is BCD)
    let sec = bcd_to_bin(sec);
    let min = bcd_to_bin(min);
    let hour = bcd_to_bin(hour);
    let day = bcd_to_bin(day);
    let month = bcd_to_bin(month);
    let year = bcd_to_bin(year) + 2000; // CMOS year is 0-99 → 2000-2099

    // Rough Unix timestamp calculation (not perfectly accurate for all months)
    let days_since_epoch = (year - 1970) * 365
        + (year - 1969) / 4  // leap years
        + month_days(month)
        + day
        - 1;

    days_since_epoch * 86400 + hour * 3600 + min * 60 + sec
}

fn month_days(month: u64) -> u64 {
    const CUMULATIVE: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    if month >= 1 && month <= 12 {
        CUMULATIVE[(month - 1) as usize]
    } else {
        0
    }
}

fn bcd_to_bin(bcd: u64) -> u64 {
    ((bcd >> 4) & 0x0F) * 10 + (bcd & 0x0F)
}

fn read_cmos(reg: u8) -> u8 {
    unsafe {
        let mut addr_port = Port::<u8>::new(0x70);
        let mut data_port = Port::<u8>::new(0x71);
        addr_port.write(reg);
        data_port.read()
    }
}
