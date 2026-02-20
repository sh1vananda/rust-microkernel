use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// A simple read-only file representation in memory.
#[derive(Debug, Clone)]
pub struct VirtualFile {
    pub name: String,
    pub data: &'static [u8],
}

struct VfsRegistry {
    files: Vec<VirtualFile>,
}

impl VfsRegistry {
    const fn new() -> Self {
        VfsRegistry { files: Vec::new() }
    }
}

static VFS: Mutex<VfsRegistry> = Mutex::new(VfsRegistry::new());

/// Register a file into the Virtual File System.
pub fn register_file(name: &str, data: &'static [u8]) {
    let mut reg = VFS.lock();
    reg.files.push(VirtualFile {
        name: String::from(name),
        data,
    });
}

/// Retrieve a file's contents by its exact name path.
pub fn open_file(name: &str) -> Option<&'static [u8]> {
    let reg = VFS.lock();
    reg.files.iter().find(|f| f.name == name).map(|f| f.data)
}

/// List all files registered in the VFS.
pub fn list_files() -> Vec<String> {
    let reg = VFS.lock();
    reg.files.iter().map(|f| f.name.clone()).collect()
}
