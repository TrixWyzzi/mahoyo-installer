fn main() {
    println!("cargo:rerun-if-changed=sources/launcher.ico");
    println!("cargo:rerun-if-changed=sources/app.manifest");
    winresource::WindowsResource::new()
        .set_icon("sources/launcher.ico")
        .set_manifest_file("sources/app.manifest")
        .set("ProductName", "Installateur Mahoyo")
        .set("FileDescription", "Installateur Mahoyo")
        .set("CompanyName", "Nolua Corp")
        .set("OriginalFilename", "Installer-Mahoyo.exe")
        .compile()
        .expect("Compilation des ressources Windows");
}
