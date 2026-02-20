#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern "C" {
    fn debug_log(ptr: *const u8, len: usize);
}

#[no_mangle]
pub extern "C" fn _start() {
    let msg = b"Hello from pure Rust Wasm Sandbox!";
    unsafe {
        debug_log(msg.as_ptr(), msg.len());
    }
}
