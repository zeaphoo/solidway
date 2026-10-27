fn main() {
    // release 构建会把 web/dist 编进二进制；产物缺失时宁可编译失败，
    // 也不要发一个"能启动但控制台 404"的二进制。
    if cfg!(not(debug_assertions)) {
        let index = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist/index.html");
        assert!(
            index.exists(),
            "未找到 web/dist/index.html；发布构建前请先执行 pnpm build"
        );
    }
    println!("cargo:rerun-if-changed=build.rs");
}
