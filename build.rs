// Embeds the application icon into the Windows executable so Explorer, the taskbar and the
// Start Menu show it without a shortcut.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon/agent-globe.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/agent-globe.ico");
        res.set("ProductName", "Agent Globe");
        res.set("FileDescription", "Agent Globe: live 3D view of coding-agent sessions");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon resource not embedded: {e}");
        }
    }
}
