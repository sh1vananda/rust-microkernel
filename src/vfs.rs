use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// A file in the Virtual File System.
/// Files from initramfs are read-only (`owner_pid = 0`).
/// Files created by agents are owned and access-controlled.
#[derive(Debug, Clone)]
pub struct VirtualFile {
    pub name: String,
    pub data: Vec<u8>,
    pub owner_pid: u64, // 0 = system/initramfs
    pub read_only: bool,
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

/// Register a read-only system file (used by initramfs loader).
pub fn register_file(name: &str, data: &'static [u8]) {
    let mut reg = VFS.lock();
    reg.files.push(VirtualFile {
        name: String::from(name),
        data: data.to_vec(),
        owner_pid: 0,
        read_only: true,
    });
}

/// Retrieve a file's contents by name.
pub fn open_file(name: &str) -> Option<Vec<u8>> {
    let reg = VFS.lock();
    reg.files
        .iter()
        .find(|f| f.name == name)
        .map(|f| f.data.clone())
}

/// List all file names in the VFS.
pub fn list_files() -> Vec<String> {
    let reg = VFS.lock();
    reg.files.iter().map(|f| f.name.clone()).collect()
}

/// List files matching a path prefix.
pub fn list_files_prefix(prefix: &str) -> Vec<String> {
    let reg = VFS.lock();
    reg.files
        .iter()
        .filter(|f| f.name.starts_with(prefix))
        .map(|f| f.name.clone())
        .collect()
}

/// Write or overwrite a file in the VFS. Returns true on success.
pub fn write_file(name: &str, data: &[u8], owner_pid: u64) -> bool {
    let mut reg = VFS.lock();

    // Check if file exists
    if let Some(existing) = reg.files.iter_mut().find(|f| f.name == name) {
        if existing.read_only {
            return false; // Cannot overwrite system files
        }
        existing.data = data.to_vec();
        existing.owner_pid = owner_pid;
        return true;
    }

    // Create new file
    reg.files.push(VirtualFile {
        name: String::from(name),
        data: data.to_vec(),
        owner_pid,
        read_only: false,
    });
    true
}

/// Delete a file from the VFS. Returns true if deleted.
pub fn delete_file(name: &str) -> bool {
    let mut reg = VFS.lock();
    let before = reg.files.len();
    reg.files.retain(|f| f.name != name || f.read_only);
    reg.files.len() < before
}
