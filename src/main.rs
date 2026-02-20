#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![allow(dead_code)]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;

mod allocator;
mod capability;
pub mod dns;
mod gdt;
pub mod initramfs;
mod interrupts;
mod ipc;
mod memory;
pub mod net;
pub mod pci;
pub mod rtl8139;
mod serial;
pub mod syscall_errors;
mod task;
pub mod time;
pub mod vfs;
mod vga_buffer;
mod wasm;

entry_point!(kernel_main);

// ── helpers ──────────────────────────────────────────────────────────────────

/// Print to both VGA screen and QEMU serial (stdout when -serial stdio).
macro_rules! log {
    ($($arg:tt)*) => {{
        println!($($arg)*);
        serial_println!($($arg)*);
    }};
}

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use memory::BootInfoFrameAllocator;
    use x86_64::VirtAddr;

    // Initialize core systems
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    // Initialize memory
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    // Initialize microkernel subsystems
    capability::init();
    ipc::init();

    log!("[SETUP] Scanning PCI buses...");
    let devices = pci::scan_buses();
    for dev in devices {
        log!(
            "  [PCI] Found Device {:04X}:{:04X} at {}:{}:{} (BAR0: {:#X})",
            dev.vendor_id,
            dev.device_id,
            dev.bus,
            dev.device,
            dev.function,
            dev.bar0
        );

        if dev.vendor_id == 0x10EC && dev.device_id == 0x8139 {
            log!("  [NET] Initializing RTL8139 Driver...");
            let io_base = (dev.bar0 & !3) as u16; // Port I/O addresses have lowest bits set as flags
            let mut rtl = rtl8139::Rtl8139::new(io_base, boot_info.physical_memory_offset);
            rtl.init();
            net::init(rtl);
        }
    }

    run_wasm_demo();
}

// ── Wasm Microvisor demo ───────────────────────────────────────────────────

fn run_wasm_demo() -> ! {
    use alloc::vec;
    use capability::{create_capability, Capability};
    use task::spawn_agent;

    log!("");
    log!("============================================================");
    log!("  Rust Microkernel — OpenClaw Wasm Sandbox POC");
    log!("============================================================");
    log!("");

    log!("[SETUP] Initializing Wasm Runtime...");
    let runtime = wasm::WasmRuntime::new();

    log!("[SETUP] Parsing Initramfs...");
    let archive_bytes = include_bytes!("archive.tar");
    match initramfs::init(archive_bytes) {
        Ok(count) => log!("  Successfully mounted {} files from Initramfs.", count),
        Err(e) => {
            log!("[ERROR] Failed to parse Initramfs: {}", e);
            panic!("Critical Boot Failure: VFS Initialization Failed.");
        }
    }

    log!("[SETUP] Spawning OpenClaw Core Agent...");

    // Give the core agent capability to spawn other agents (skills) and use the network
    let cap_spawn = create_capability(Capability::Spawn { max_children: 10 });
    let cap_net = create_capability(Capability::Network);
    let core_agent = spawn_agent("openclaw_core", vec![cap_spawn, cap_net]);
    let pid = task::agent_pid(core_agent);

    log!("  Agent 'openclaw_core' created with PID: {}", pid);

    let mut executed_count = 0;
    let files = vfs::list_files();

    for filename in files {
        if filename.ends_with(".wasm") {
            log!("[EXEC] Found Wasm Agent: {}", filename);
            if let Some(wasm_bytes) = vfs::open_file(&filename) {
                log!("  Executing {}...", filename);
                match runtime.execute_module(&wasm_bytes, pid) {
                    Ok(_) => {
                        log!("  [SUCCESS] {} executed successfully.", filename);
                    }
                    Err(e) => {
                        log!("  [ERROR] {} execution failed: {}", filename, e);
                    }
                }
                executed_count += 1;
            }
        }
    }

    if executed_count == 0 {
        log!("[WARN] No .wasm files found in the VFS.");
    }

    log!("");
    log!("============================================================");
    log!("  Microvisor Halted.");
    log!("============================================================");
    log!("");

    loop {
        x86_64::instructions::hlt();
    }
}

// ── Required handlers ─────────────────────────────────────────────────────────

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("KERNEL PANIC: {}", info);
    println!("KERNEL PANIC: {}", info);
    loop {
        x86_64::instructions::hlt();
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}
