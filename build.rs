fn main() {
    // Check for libxkbcommon at compile time and provide a helpful error message
    match pkg_config::Config::new().probe("xkbcommon") {
        Ok(_) => {}
        Err(e) => {
            eprintln!("\n╔══════════════════════════════════════════════════════════════════╗");
            eprintln!("║                    Missing System Dependency                      ║");
            eprintln!("╠══════════════════════════════════════════════════════════════════╣");
            eprintln!("║  libxkbcommon is required to build eitype.                        ║");
            eprintln!("║                                                                   ║");
            eprintln!("║  Install it using your package manager:                           ║");
            eprintln!("║                                                                   ║");
            eprintln!("║    Debian/Ubuntu:  sudo apt install libxkbcommon-dev              ║");
            eprintln!("║    Fedora/RHEL:    sudo dnf install libxkbcommon-devel            ║");
            eprintln!("║    Arch Linux:     sudo pacman -S libxkbcommon                    ║");
            eprintln!("║    openSUSE:       sudo zypper install libxkbcommon-devel         ║");
            eprintln!("║                                                                   ║");
            eprintln!("╚══════════════════════════════════════════════════════════════════╝\n");
            panic!("pkg-config failed to find libxkbcommon: {}", e);
        }
    }
}
