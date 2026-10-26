fn main() {
    #[cfg(windows)]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/app.ico");
        resource.set("ProductName", "Singularity");
        resource.set("FileDescription", "奇点 · 黑洞桌宠");
        resource.set("CompanyName", "Singularity");
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.5.0".to_owned());
        resource.set("FileVersion", &format!("{version}.0"));
        resource.set("ProductVersion", &version);
        resource.set("OriginalFilename", "Singularity.exe");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=winres failed: {error}");
        }
    }
}
