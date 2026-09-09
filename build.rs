fn main() {
    #[cfg(windows)]
    winres::WindowsResource::new()
        .set_icon("assets/opencast.ico")
        .set("ProductName", "OpenCast")
        .set("FileDescription", "OpenCast — file search and calculator")
        .compile()
        .expect("Windows icon resource compilation failed");
}
