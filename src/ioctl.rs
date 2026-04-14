#![allow(non_snake_case)]
use std::os::raw::{c_uint, c_ulong};

// These values are from https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/ioctl.h
const _IOC_NRBITS: c_uint = 8;
const _IOC_TYPEBITS: c_uint = 8;
const _IOC_SIZEBITS: c_uint = 14;
const _IOC_DIRBITS: c_uint = 2;

const _IOC_NRSHIFT: c_uint = 0;
const _IOC_TYPESHIFT: c_uint = _IOC_NRSHIFT + _IOC_NRBITS;
const _IOC_SIZESHIFT: c_uint = _IOC_TYPESHIFT + _IOC_TYPEBITS;
const _IOC_DIRSHIFT: c_uint = _IOC_SIZESHIFT + _IOC_SIZEBITS;

const _IOC_NONE: c_uint = 0;
const _IOC_WRITE: c_uint = 1;
const _IOC_READ: c_uint = 2;

pub const fn _IOC(dir: c_uint, _type: c_uint, nr: c_uint, size: c_uint) -> c_ulong {
    ((dir << _IOC_DIRSHIFT)
        | (_type << _IOC_TYPESHIFT)
        | (nr << _IOC_NRSHIFT)
        | (size << _IOC_SIZESHIFT)) as c_ulong
}

pub const fn _IO(_type: c_uint, nr: c_uint) -> c_ulong {
    _IOC(_IOC_NONE, _type, nr, 0)
}

pub const fn _IOR(_type: c_uint, nr: c_uint, size: c_uint) -> c_ulong {
    _IOC(_IOC_READ, _type, nr, size)
}

pub const fn _IOW(_type: c_uint, nr: c_uint, size: c_uint) -> c_ulong {
    _IOC(_IOC_WRITE, _type, nr, size)
}

pub const fn _IOWR(_type: c_uint, nr: c_uint, size: c_uint) -> c_ulong {
    _IOC(_IOC_READ | _IOC_WRITE, _type, nr, size)
}
