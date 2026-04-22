# miniHype

## Linux KVM

[minimal-x86](./minimal-x86/) Uses KVM API to run a bare minimum x86 VM

Run VM on x86_64 Linux:
```bash
cd minimal-x86
cargo run
```

## Apple Hypervisor

[minimal-apple-hypervisor](./minimal-apple-hypervisor/) Uses Apple Hypervisor to run a bare minimum arm64 VM

Run VM On Apple Silicon macOS:
```bash
cd minimal-apple-hypervisor
cargo build
# the binary must have the entitlement
codesign --sign - --force --entitlements=virt.entitlements ../target/debug/minimal-apple-hypervisor
# Run the binary
../target/debug/minimal-apple-hypervisor
```