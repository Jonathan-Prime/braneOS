use std::process::Command;

fn main() {
    // Read the kernel path passed by the Makefile
    let kernel_path =
        std::env::var("KERNEL_BIN_PATH").expect("KERNEL_BIN_PATH environment variable not set");

    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| "target".into());
    let uefi_path = format!("{}/brane_os-uefi.img", out_dir);
    let bios_path = format!("{}/brane_os-bios.img", out_dir);

    println!("Building BIOS boot image...");
    bootloader::BiosBoot::new(kernel_path.as_ref())
        .create_disk_image(bios_path.as_ref())
        .unwrap();

    println!("Building UEFI boot image...");
    bootloader::UefiBoot::new(kernel_path.as_ref())
        .create_disk_image(uefi_path.as_ref())
        .unwrap();

    println!(
        "Bootable disk images created at:\n  BIOS: {}\n  UEFI: {}",
        bios_path, uefi_path
    );

    // Launch QEMU with the UEFI image
    println!("Launching QEMU (UEFI mode)...");

    let mut qemu = Command::new("qemu-system-x86_64")
        .arg("-drive")
        .arg("if=pflash,format=raw,readonly=on,file=/usr/local/Cellar/qemu/10.2.1/share/qemu/edk2-x86_64-code.fd")
        .arg("-drive")
        .arg(format!("format=raw,file={}", uefi_path))
        .arg("-accel")
        .arg("hvf")
        .arg("-serial")
        .arg("file:kernel_serial.log")
        .arg("-display")
        .arg("none")
        .arg("-m")
        .arg("128M")
        .arg("-netdev")
        .arg("user,id=n0")
        .arg("-device")
        .arg("virtio-net-pci,netdev=n0")
        .spawn()
        .expect("Failed to start QEMU");

    qemu.wait().unwrap();
}
