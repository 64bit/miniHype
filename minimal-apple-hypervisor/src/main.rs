#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unnecessary_transmutes)]
#![allow(improper_ctypes)]
#![allow(unused)]

use std::error::Error;
use std::fmt::Display;

use crate::bindings::{
    HV_BAD_ARGUMENT, HV_BUSY, HV_ERROR, HV_MEMORY_EXEC, HV_MEMORY_READ, HV_MEMORY_WRITE,
    HV_NO_DEVICE, HV_NO_RESOURCES, HV_SUCCESS, HV_UNSUPPORTED, hv_reg_t_HV_REG_PC,
    hv_vcpu_config_create, hv_vcpu_create, hv_vcpu_exit_t, hv_vcpu_run, hv_vcpu_set_reg,
    hv_vm_config_create, hv_vm_create, hv_vm_destroy, hv_vm_map, hv_vm_unmap, os_release,
};

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

const VM_MEMORY_SIZE: usize = 2 * 16384; // 2 blocks of 16KiB
const CODE: [u8; 4] = [0xD5, 0x03, 0x20, 0x7F]; // WFI instruction

struct Mmap {
    pub addr: *mut std::os::raw::c_void,
    pub len: usize,
}

impl Mmap {
    pub fn new(len: usize) -> Result<Self, Box<dyn Error>> {
        let addr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                VM_MEMORY_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANON | libc::MAP_PRIVATE,
                -1,
                0,
            )
        };

        if addr == libc::MAP_FAILED {
            return Err(format!("failed to mmap: {}", std::io::Error::last_os_error()).into());
        }

        Ok(Mmap { addr, len })
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        if !self.addr.is_null() {
            unsafe {
                libc::munmap(self.addr, self.len);
            }
        }
    }
}

#[derive(Debug)]
pub struct HVError(i32);
impl std::error::Error for HVError {}

impl Display for HVError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl HVError {
    pub fn from_error(err: i32) -> Self {
        HVError(err)
    }
}

macro_rules! hv_call {
    ($e:expr) => {{
        let val = unsafe { $e };

        match val {
            HV_SUCCESS => println!("HV_SUCCESS: The operation completed successfully."),
            HV_ERROR => eprintln!("HV_ERROR: The operation was unsuccessful."),
            HV_BUSY => eprintln!(
                "HV_BUSY: The operation was unsuccessful because the owning resource was busy."
            ),
            HV_BAD_ARGUMENT => eprintln!(
                "HV_BAD_ARGUMENT: The operation was unsuccessful because the function call had an invalid argument."
            ),
            HV_NO_RESOURCES => eprintln!(
                "HV_NO_RESOURCES: The operation was unsuccessful because the host had no resources available to complete the request."
            ),
            HV_NO_DEVICE => eprintln!(
                "HV_NO_DEVICE: The operation was unsuccessful because no VM or vCPU was available."
            ),
            HV_UNSUPPORTED => {
                eprintln!("HV_UNSUPPORTED: The operation requested isn’t supported by the hypervisor.")
            }
            _ => eprintln!("unknown {val}"),
        }

        match val {
            HV_SUCCESS => Ok(val),
            _ => Err(HVError::from_error(val))
        }
    }};
}

fn main() -> Result<(), Box<dyn Error>> {
    let vm = hv_call!(hv_vm_create(std::ptr::null_mut()))?;

    let vm_mmap = Mmap::new(VM_MEMORY_SIZE)?;

    let vm_map = hv_call!(hv_vm_map(
        vm_mmap.addr,
        0,
        VM_MEMORY_SIZE,
        (HV_MEMORY_READ | HV_MEMORY_WRITE | HV_MEMORY_EXEC).into(),
    ))?;

    unsafe { std::ptr::copy_nonoverlapping(&CODE as *const u8, vm_mmap.addr as *mut u8, 4); }

    let mut vcpu_exit = std::ptr::null_mut();
    let mut id = 0;
    let vcpu = hv_call!(hv_vcpu_create(
        &mut id,
        &mut vcpu_exit,
        std::ptr::null_mut()
    ))?;

    let set_reg = hv_call!(hv_vcpu_set_reg(id, hv_reg_t_HV_REG_PC, 0))?;

    loop {
        let run = hv_call!(hv_vcpu_run(id))?;
        println!("exit from run: {run}");
    }

    let vm_unmap = hv_call!(hv_vm_unmap(0, VM_MEMORY_SIZE))?;

    let vm_destroy = hv_call!(hv_vm_destroy())?;

    Ok(())
}
