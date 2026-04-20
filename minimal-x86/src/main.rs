use std::{
    error::Error,
    fs::OpenOptions,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::OpenOptionsExt,
    },
};

use kvm_bindings::{kvm_regs, kvm_run, kvm_sregs, kvm_userspace_memory_region};
use std::os::raw::{c_uint, c_ulong};

const KVM_VERSION: i32 = 12;
const KVMIO: c_uint = 0xAE;
const KVM_GET_API_VERSION: c_ulong = libc::_IO(KVMIO, 0x00);
const KVM_CREATE_VM: c_ulong = libc::_IO(KVMIO, 0x01);
const KVM_GET_VCPU_MMAP_SIZE: c_ulong = libc::_IO(KVMIO, 0x04);
const KVM_CREATE_VCPU: c_ulong = libc::_IO(KVMIO, 0x41);
const KVM_SET_USER_MEMORY_REGION: c_ulong = libc::_IOW::<kvm_userspace_memory_region>(KVMIO, 0x46);

const KVM_RUN: c_ulong = libc::_IO(KVMIO, 0x80);
const KVM_SET_REGS: c_ulong = libc::_IOW::<kvm_regs>(KVMIO, 0x82);
const KVM_GET_SREGS: c_ulong = libc::_IOR::<kvm_sregs>(KVMIO, 0x83);
const KVM_SET_SREGS: c_ulong = libc::_IOW::<kvm_sregs>(KVMIO, 0x84);

const VM_MEMORY: u64 = 2 * 4096; // 2 blocks of 4KiB

const CODE: [u8; 1] = [0xF4]; // HLT

struct Mmap {
    pub ptr: *mut std::os::raw::c_void,
    pub len: usize,
}

impl Drop for Mmap {
    fn drop(&mut self) {
        println!("calling libc:munmap");
        unsafe { libc::munmap(self.ptr, self.len) };
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_RDWR | libc::O_CLOEXEC)
        .open("/dev/kvm")?;

    let kvm_fd = file.as_raw_fd();

    //
    // 1. Check KVM version, it not 12 refuse to run.
    //

    let kvm_version = unsafe { libc::ioctl(kvm_fd, KVM_GET_API_VERSION, 0) };

    println!("kvm version {kvm_version}");

    if kvm_version < 0 {
        let last_os_error = std::io::Error::last_os_error();
        println!("error getting kvm version");
        return Err(last_os_error.into());
    }

    if kvm_version != KVM_VERSION {
        eprintln!("current kvm version: {kvm_version}, required kvm version: {KVM_VERSION}");
        return Err("kvm version not supported".into());
    }

    //
    // 2. Create A VM
    //

    let vm_fd = unsafe { libc::ioctl(kvm_fd, KVM_CREATE_VM, 0) };

    if vm_fd < 0 {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("vm creation error");
        return Err(last_os_error.into());
    }
    // Own it so that fd is closed on drop
    let vm_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(vm_fd) };

    //
    // 3. Create A VCPU
    //

    let vcpu_fd = unsafe { libc::ioctl(vm_fd.as_raw_fd(), KVM_CREATE_VCPU, 0) };

    if vcpu_fd < 0 {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("vcpu creation error");
        return Err(last_os_error.into());
    }
    // Own it so that fd is closed on drop
    let vcpu_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(vcpu_fd) };

    println!(
        "kvm fd {kvm_fd}, vm fd: {}, vcpu fd: {}",
        vm_fd.as_raw_fd(),
        vcpu_fd.as_raw_fd()
    );

    //
    // 4. Create memory for guest
    //

    let vm_memory_mmap = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            VM_MEMORY as usize,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANONYMOUS | libc::MAP_PRIVATE,
            -1,
            0,
        )
    };

    if vm_memory_mmap == libc::MAP_FAILED {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("vm memory map failed");
        return Err(last_os_error.into());
    }

    // take ownership, so on drop munmap is called
    let vm_memory_mmap = Mmap {
        ptr: vm_memory_mmap,
        len: VM_MEMORY as usize,
    };

    //
    // 5. Copy code to guest's physical memory - that guest will execute
    //

    unsafe {
        std::ptr::copy_nonoverlapping(&CODE as *const u8, vm_memory_mmap.ptr as *mut u8, 1);
    }

    let vm_memory_addr = vm_memory_mmap.ptr as u64;
    println!("vm_memory_addr: 0x{vm_memory_addr:0x}");

    //
    // 6. Setup guest's physical memory
    //
    let userspace_memory_region = kvm_userspace_memory_region {
        slot: 0,
        flags: 0,
        guest_phys_addr: 4096,
        memory_size: VM_MEMORY,
        userspace_addr: vm_memory_addr,
    };

    let ret = unsafe {
        libc::ioctl(
            vm_fd.as_raw_fd(),
            KVM_SET_USER_MEMORY_REGION,
            &userspace_memory_region,
        )
    };

    if ret != 0 {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("error setting user memory region");
        return Err(last_os_error.into());
    }

    //
    // 7.1 Setup regular x86 cpu registers.
    //   Set instruction pointer to start execution at 2nd' block of size 4096, because that's where we copied code
    //

    let k_regs = kvm_regs {
        rip: 4096,
        rflags: 0x2,
        ..Default::default()
    };

    let ret = unsafe { libc::ioctl(vcpu_fd.as_raw_fd(), KVM_SET_REGS, &k_regs) };

    if ret != 0 {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("error setting kvm_regs");
        return Err(last_os_error.into());
    }

    //
    // 7.2 Read default x86 special registers, and update them
    //

    let mut k_sregs = kvm_sregs::default();
    let ret = unsafe { libc::ioctl(vcpu_fd.as_raw_fd(), KVM_GET_SREGS, &k_sregs) };

    if ret != 0 {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("error getting kvm_sregs");
        return Err(last_os_error.into());
    }

    k_sregs.cs.base = 0;
    k_sregs.cs.selector = 0;

    let ret = unsafe { libc::ioctl(vcpu_fd.as_raw_fd(), KVM_SET_SREGS, &k_sregs) };

    if ret != 0 {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("error setting kvm_sregs");
        return Err(last_os_error.into());
    }

    //
    // 8.1. Get the size of kvm_run
    //

    let vcpu_mmap_size = unsafe { libc::ioctl(kvm_fd, KVM_GET_VCPU_MMAP_SIZE, 0) };

    if vcpu_mmap_size < 0 {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("error getting vcpu mmap size: {vcpu_mmap_size}");
        return Err(last_os_error.into());
    }

    println!("vcpu mmap size: {vcpu_mmap_size} bytes");

    //
    // 8.2 memory map the pointer to kvm_run data structure
    //

    let kvm_run_mmap = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            vcpu_mmap_size as usize,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            vcpu_fd.as_raw_fd(),
            0,
        )
    };

    if kvm_run_mmap == libc::MAP_FAILED {
        let last_os_error = std::io::Error::last_os_error();
        eprintln!("kvm_run mmap failed");
        return Err(last_os_error.into());
    }

    // take ownership, so on drop munmap is called
    let kvm_run_mmap = Mmap {
        ptr: kvm_run_mmap,
        len: vcpu_mmap_size as usize,
    };

    //
    // 9. Run VM until it executes hlt instruction in CODE
    //
    loop {
        let ret = unsafe { libc::ioctl(vcpu_fd.as_raw_fd(), KVM_RUN, 0) };

        if ret != 0 {
            eprintln!("KVM_RUN errored")
        }

        let k_run: &kvm_run = unsafe { &*(kvm_run_mmap.ptr as *const kvm_run) };

        match k_run.exit_reason {
            kvm_bindings::KVM_EXIT_HLT => {
                println!("KVM_EXIT_HTL");
                return Ok(());
            }
            _ => {
                eprintln!("EXIT: {:?}", k_run);
            }
        }
    }
}
