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
    HV_NO_DEVICE, HV_NO_RESOURCES, HV_SUCCESS, HV_UNSUPPORTED, hv_reg_t_HV_REG_CPSR,
    hv_reg_t_HV_REG_PC, hv_reg_t_HV_REG_X0, hv_sys_reg_t_HV_SYS_REG_HCR_EL2, hv_vcpu_config_create,
    hv_vcpu_create, hv_vcpu_destroy, hv_vcpu_exit_t, hv_vcpu_get_reg, hv_vcpu_run, hv_vcpu_set_reg,
    hv_vcpu_set_sys_reg, hv_vm_config_create, hv_vm_create, hv_vm_destroy, hv_vm_map, hv_vm_unmap,
    os_release,
};

use std::arch::global_asm;

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

const VM_MEMORY_SIZE: usize = 2 * 16384; // 2 blocks of 16KiB

global_asm!(
    r#"
    .global _guest_code_start
    .global _guest_code_end
    .align 4
_guest_code_start:
    mov x0, #0x41   // Move a value in general purpose register, so we can see it after vcpu exit.
    hvc #0          // Hypervisor call - will cause exit
_guest_code_end:
    "#
);

unsafe extern "C" {
    static guest_code_start: u8;
    static guest_code_end: u8;
}

fn guest_code() -> &'static [u8] {
    unsafe {
        let start = &guest_code_start as *const u8;
        let end = &guest_code_end as *const u8;
        let len = end.offset_from(start) as usize;
        std::slice::from_raw_parts(start, len)
    }
}

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

    unsafe {
        let code = guest_code();
        std::ptr::copy_nonoverlapping(code.as_ptr(), vm_mmap.addr as *mut u8, code.len());
    }

    let mut vcpu_exit = std::ptr::null_mut();
    let mut id = 0;
    let vcpu = hv_call!(hv_vcpu_create(
        &mut id,
        &mut vcpu_exit,
        std::ptr::null_mut()
    ))?;

    let set_reg = hv_call!(hv_vcpu_set_reg(id, hv_reg_t_HV_REG_PC, 0))?;

    // Set Program State Register to basically disable all interrupts.
    // Put the CPU in EL1h (EL1 using SP_EL1) with interrupts masked. Value 0x3c5:
    // bits [3:0] = 0101 → EL1h mode
    // bit 6 (F) = 1 → mask FIQ
    // bit 7 (I) = 1 → mask IRQ
    // bit 8 (A) = 1 → mask SError
    // bit 9 (D) = 1 → mask debug
    hv_call!(hv_vcpu_set_reg(id, hv_reg_t_HV_REG_CPSR, 0x3c5))?;

    loop {
        hv_call!(hv_vcpu_run(id))?;
        let exit = unsafe { &*vcpu_exit };
        println!("exit reason: {}", exit.reason);
        println!("  physical_address: {:#x}", exit.exception.physical_address);
        println!("  virtual_address: {:#x}", exit.exception.virtual_address);
        println!("  syndrome: {:#x}", exit.exception.syndrome);

        // check that HVC got us here
        // https://developer.arm.com/documentation/ddi0602/2022-09/Base-Instructions/HVC--Hypervisor-Call-
        // https://developer.arm.com/documentation/111107/2026-03/AArch64-Registers/ESR-EL1--Exception-Syndrome-Register--EL1-
        // exception class bits [31:26]
        let exception_class = (exit.exception.syndrome >> 26) & 0x3f;
        if exception_class == 0x16 {
            println!("HVC executed in Guest");
            // get the value of x0
            let mut x0 = 0u64;
            hv_call!(hv_vcpu_get_reg(id, hv_reg_t_HV_REG_X0, &mut x0))?;
            println!("X0 from Guest: {x0:#x}");
        }

        break;
    }

    let vcpu_destroy = hv_call!(hv_vcpu_destroy(id))?;
    let vm_unmap = hv_call!(hv_vm_unmap(0, VM_MEMORY_SIZE))?;
    let vm_destroy = hv_call!(hv_vm_destroy())?;

    Ok(())
}
