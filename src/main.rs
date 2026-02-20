#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![allow(dead_code)]

extern crate alloc;

use core::panic::PanicInfo;
use bootloader::{BootInfo, entry_point};

mod vga_buffer;
mod serial;
mod interrupts;
mod gdt;
mod memory;
mod allocator;
mod capability;
mod ipc;
mod task;
mod wasm;
pub mod vfs;
pub mod initramfs;

entry_point!(kernel_main);

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
    let mut frame_allocator = unsafe {
        BootInfoFrameAllocator::init(&boot_info.memory_map)
    };
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    // Initialize microkernel subsystems
    capability::init();
    ipc::init();

    run_wasm_demo();
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Print to both VGA screen and QEMU serial (stdout when -serial stdio).
macro_rules! log {
    ($($arg:tt)*) => {{
        println!($($arg)*);
        serial_println!($($arg)*);
    }};
}

// ── Wasm Microvisor demo ───────────────────────────────────────────────────

fn run_wasm_demo() -> ! {
    use alloc::vec;
    use capability::{Capability, create_capability};
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
    
    // Give the core agent capability to spawn other agents (skills)
    let cap_spawn = create_capability(Capability::Spawn { max_children: 10 });
    let core_agent = spawn_agent("openclaw_core", vec![cap_spawn]);
    let pid = task::agent_pid(core_agent);

    log!("  Agent 'openclaw_core' created with PID: {}", pid);

    let mut executed_count = 0;
    let files = vfs::list_files();
    
    for filename in files {
        if filename.ends_with(".wasm") {
            log!("[EXEC] Found Wasm Agent: {}", filename);
            if let Some(wasm_bytes) = vfs::open_file(&filename) {
                log!("  Executing {}...", filename);
                match runtime.execute_module(wasm_bytes, pid) {
                    Ok(_) => { log!("  [SUCCESS] {} executed successfully.", filename); }
                    Err(e) => { log!("  [ERROR] {} execution failed: {}", filename, e); }
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
