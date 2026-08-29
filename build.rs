//! 编译期嵌入 Windows 资源（exe 图标等）；仅 Windows 目标生效。

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("嵌入 Windows 资源（icon.ico）失败");
    }
}
