//! 编译期嵌入 Windows 资源（图标/清单/版本信息）；仅 Windows 目标生效。

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            // 应用清单：声明 Common Controls v6（nwg 的 SetWindowSubclass 依赖）、
            // Win7~11 兼容性、DPI 感知——缺清单时 Win7+ 默认加载 comctl32 v5
            .set_manifest(include_str!("resources/app.manifest"))
            .compile()
            .expect("嵌入 Windows 资源（icon/manifest）失败");
    }
}
