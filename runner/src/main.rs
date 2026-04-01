use std::process::Command;
use std::env;

fn main() {
    // Read the kernel path passed by the Makefile
    let kernel_path =
        env::var("KERNEL_BIN_PATH").expect("KERNEL_BIN_PATH environment variable not set");

    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".into());
    let uefi_path = format!("{}/brane_os-uefi.img", out_dir);
    let bios_path = format!("{}/brane_os-bios.img", out_dir);

    // Ensure the output directory exists
    std::fs::create_dir_all(&out_dir).ok();

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

    // Launch QEMU
    println!("Launching QEMU...");

    let mut qemu = Command::new("qemu-system-x86_64");

    // Select acceleration based on OS
    match env::consts::OS {
        "windows" => {
            qemu.arg("-accel").arg("whpx").arg("-accel").arg("tcg");
        }
        "macos" => {
            qemu.arg("-accel").arg("hvf").arg("-accel").arg("tcg");
        }
        "linux" => {
            qemu.arg("-accel").arg("kvm").arg("-accel").arg("tcg");
        }
        _ => {
            qemu.arg("-accel").arg("tcg");
        }
    }

    // Use BIOS image by default for better compatibility across platforms 
    // unless an EFI firmware path is provided.
    qemu.arg("-drive").arg(format!("format=raw,file={}", bios_path));
    
    qemu.arg("-m").arg("256M");
    qemu.arg("-serial").arg("stdio"); // Redirect serial to terminal
    
    // Networking
    qemu.arg("-netdev").arg("user,id=n0");
    qemu.arg("-device").arg("virtio-net-pci,netdev=n0");

    let mut child = qemu.spawn().expect("Failed to start QEMU. Is it installed and in your PATH?");
    child.wait().unwrap();
}
